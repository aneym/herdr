//! Filesystem-only agent home resolution, polled by the server scheduler.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::api::schema::{HomeLocation, TabAgentCard};

struct Card {
    pane: Option<String>,
    card: TabAgentCard,
}

fn parse_card(bytes: &[u8]) -> Option<Card> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    let name = text("name")?;
    let pane = text("pane").filter(|pane| pane != "none");
    let avatar_url = text("avatar_url").filter(|url| {
        url.get(..8)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
    });
    Some(Card {
        pane,
        card: TabAgentCard { name, avatar_url },
    })
}

fn parse_marker(bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .is_some_and(|value| value.is_object())
}

fn parse_status(bytes: &[u8]) -> HomeLocation {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return HomeLocation::Unsynced;
    };
    if value["schema"] == 1 && value["location"] == "cloud" {
        HomeLocation::Cloud
    } else {
        HomeLocation::Unsynced
    }
}

/// The key a folder's location is also stored under, for an agent tab whose
/// pane no card names: `agent:<folder>`, which no public pane id can equal.
pub(crate) fn folder_key(name: &str) -> String {
    format!("agent:{name}")
}

/// The folder an agent tab's label names: "League Coach" -> "league-coach".
fn label_folder(label: &str) -> String {
    label
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

pub(crate) fn label_folders(label: &str) -> Vec<String> {
    let mut folders = vec![label_folder(label)];
    if let Some((head, _)) = label.split_once(" · ") {
        let head = label_folder(head);
        if !head.is_empty() && !folders.contains(&head) {
            folders.push(head);
        }
    }
    folders
}

#[derive(Default)]
struct CachedFile {
    stamp: Option<(Option<SystemTime>, u64)>,
    bytes: Option<Vec<u8>>,
    exists: bool,
}

#[derive(Default)]
pub(crate) struct AgentHomePoller {
    files: HashMap<PathBuf, CachedFile>,
    homes: HashMap<String, HomeLocation>,
    cards: HashMap<String, TabAgentCard>,
    file_reads: u64,
}

impl AgentHomePoller {
    fn file(&mut self, path: &Path) -> &CachedFile {
        let metadata = std::fs::metadata(path);
        let exists = match &metadata {
            Ok(_) => true,
            Err(err) => err.kind() != std::io::ErrorKind::NotFound,
        };
        let stamp = metadata.as_ref().ok().map(|m| (m.modified().ok(), m.len()));
        let cached = self.files.entry(path.to_owned()).or_default();
        if cached.stamp != stamp {
            cached.stamp = stamp;
            cached.bytes = if stamp.is_some() {
                self.file_reads += 1;
                std::fs::read(path).ok()
            } else {
                None
            };
        }
        // A permission failure is not evidence that a cloud marker was removed.
        cached.exists = exists;
        cached
    }

    pub(crate) fn poll(&mut self, agents_dir: &Path) -> bool {
        let mut folders: Vec<_> = std::fs::read_dir(agents_dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .collect();
        folders.sort_by_key(|entry| entry.file_name());
        let mut homes = HashMap::new();
        let mut cards = HashMap::new();
        let mut visited = HashSet::new();
        for entry in folders {
            let folder = entry.path();
            let card = folder.join("agent.json");
            let marker = folder.join(".rails/home.json");
            let status = folder.join(".rails/status.json");
            visited.extend([card.clone(), marker.clone(), status.clone()]);
            let card = self.file(&card).bytes.as_deref().and_then(parse_card);
            let marker = self.file(&marker);
            let has_marker = marker.exists;
            let valid_marker = marker.bytes.as_deref().is_some_and(parse_marker);
            let status = self.file(&status).bytes.as_deref().map(parse_status);
            // agent-home's per-file homes keep no home.json, only status.json, so a
            // status file alone names the location; a marker with no status is unsynced.
            let location = match status {
                Some(status) if !has_marker || valid_marker => status,
                _ if has_marker || status.is_some() => HomeLocation::Unsynced,
                _ => HomeLocation::Local,
            };
            if let Some(pane) = card.as_ref().and_then(|card| card.pane.clone()) {
                homes.entry(pane.clone()).or_insert(location);
                if let Some(card) = &card {
                    cards.entry(pane).or_insert_with(|| card.card.clone());
                }
            }
            // A pinned agent tab with no card pane finds its home by folder name.
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                if let Some(name) = entry.file_name().to_str() {
                    homes.insert(folder_key(name), location);
                    if let Some(card) = card {
                        cards.insert(folder_key(name), card.card);
                    }
                }
            }
        }
        self.files.retain(|path, _| visited.contains(path));
        if self.homes == homes && self.cards == cards {
            false
        } else {
            self.homes = homes;
            self.cards = cards;
            true
        }
    }

    pub(crate) fn homes(&self) -> &HashMap<String, HomeLocation> {
        &self.homes
    }

    pub(crate) fn cards(&self) -> &HashMap<String, TabAgentCard> {
        &self.cards
    }

    #[cfg(test)]
    pub(crate) fn file_reads(&self) -> u64 {
        self.file_reads
    }
}

#[cfg(test)]
mod tests;
