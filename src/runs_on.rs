//! Where an agent chat runs, resolved by the server scheduler for `TabInfo.runs_on`.
//!
//! A pane whose Claude session moved onto Alex's Rails box (the move runner's
//! `~/.agent-rails/rails-host/adopted.json`, entry `target: "box"`) runs on "box".
//! A pane whose foreground job is `ssh <host>` runs on that machine. Any other
//! pane runs on this server's machine. Names come from the factory machine
//! registry (`fleet/config/machines.json`): a machine's capitalised alias
//! ("Studio", "Book", "PC"), else its name; an unknown host keeps its own name.
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub(crate) const BOX: &str = "box";

/// Lowercased keys (name, aliases, ssh aliases, hostnames) to display names.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Registry(HashMap<String, String>);

fn host_key(raw: &str) -> String {
    let host = raw.rsplit('@').next().unwrap_or(raw);
    let host = host.split([' ', ':']).next().unwrap_or(host);
    host.trim_end_matches(".local").to_ascii_lowercase()
}

impl Registry {
    pub(crate) fn parse(bytes: &[u8]) -> Self {
        let Ok(serde_json::Value::Array(machines)) = serde_json::from_slice(bytes) else {
            return Self::default();
        };
        let mut keys = HashMap::new();
        for machine in &machines {
            let Some(name) = machine["name"].as_str().filter(|name| !name.is_empty()) else {
                continue;
            };
            let aliases: Vec<&str> = machine["aliases"]
                .as_array()
                .map(|list| list.iter().filter_map(|alias| alias.as_str()).collect())
                .unwrap_or_default();
            let display = aliases
                .first()
                .filter(|alias| alias.starts_with(|c: char| c.is_ascii_uppercase()))
                .copied()
                .unwrap_or(name);
            let hosts = ["ssh", "herdr_ssh", "tailscale"]
                .iter()
                .filter_map(|field| machine[*field].as_str());
            for key in std::iter::once(name).chain(aliases).chain(hosts) {
                keys.entry(host_key(key))
                    .or_insert_with(|| display.to_owned());
                if let Some(short) = host_key(key).split('.').next() {
                    keys.entry(short.to_owned())
                        .or_insert_with(|| display.to_owned());
                }
            }
        }
        Self(keys)
    }

    pub(crate) fn load(path: &Path) -> Self {
        std::fs::read(path)
            .map(|bytes| Self::parse(&bytes))
            .unwrap_or_default()
    }

    /// The registry's name for a host, else the host itself without user or domain suffix.
    pub(crate) fn name(&self, host: &str) -> String {
        let key = host_key(host);
        self.0
            .get(&key)
            .or_else(|| self.0.get(key.split('.').next().unwrap_or(&key)))
            .cloned()
            .unwrap_or_else(|| {
                let host = host.rsplit('@').next().unwrap_or(host);
                host.trim_end_matches(".local").to_owned()
            })
    }
}

/// Public pane ids whose session the move runner adopted onto the box.
pub(crate) fn box_panes(bytes: &[u8]) -> HashSet<String> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return HashSet::new();
    };
    value["panes"]
        .as_object()
        .map(|panes| {
            panes
                .iter()
                .filter(|(_, entry)| entry["target"] == BOX)
                .map(|(pane, _)| pane.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// The destination of an `ssh` command line, or None for anything else.
pub(crate) fn ssh_destination(argv: &[String]) -> Option<&str> {
    let program = argv.first()?;
    if program.rsplit('/').next() != Some("ssh") {
        return None;
    }
    // ssh(1) options that take a value as the next argument.
    const WITH_VALUE: &str = "BbcDEeFIiJLlmOoPpQRSWw";
    let mut args = argv[1..].iter();
    while let Some(arg) = args.next() {
        match arg.strip_prefix('-') {
            Some(flags) if !flags.is_empty() => {
                let first = flags.chars().next().unwrap_or_default();
                if flags.len() == 1 && WITH_VALUE.contains(first) {
                    args.next();
                }
            }
            _ => return Some(arg),
        }
    }
    None
}

#[cfg(test)]
mod tests;
