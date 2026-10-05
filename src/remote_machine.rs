//! Which machine a pane's foreground program runs on (fork feature, 2026-10-05).
//!
//! A pane whose foreground job is an interactive remote shell (`ssh ax42`,
//! `mosh book`) runs its agent on that host, not on this endpoint. The server
//! publishes the short host name on each client-shell pane so the sidebar can
//! say where a chat runs. Local work publishes nothing.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// ssh options that take a value as the next argument (OpenSSH 9).
const SSH_VALUE_FLAGS: &[char] = &[
    'B', 'b', 'c', 'D', 'E', 'e', 'F', 'I', 'i', 'J', 'L', 'l', 'm', 'O', 'o', 'P', 'p', 'Q', 'R',
    'S', 'W', 'w',
];

/// mosh long options that take a value as the next argument when written
/// without `=`.
const MOSH_VALUE_FLAGS: &[&str] = &[
    "--client",
    "--server",
    "--ssh",
    "--port",
    "-p",
    "--predict",
    "--family",
    "--bind-server",
    "--experimental-remote-ip",
];

/// Short machine label for a remote-shell leader process, or `None` when the
/// process is not `ssh`/`mosh` or names no destination.
pub(crate) fn from_argv(name: &str, argv: &[String]) -> Option<String> {
    let program = argv
        .first()
        .map(|arg0| arg0.rsplit('/').next().unwrap_or(arg0))
        .filter(|arg0| !arg0.is_empty())
        .unwrap_or(name);
    let destination = match program {
        "ssh" => ssh_destination(argv.get(1..)?),
        "mosh" => mosh_destination(argv.get(1..)?),
        // `mosh` execs `mosh-client -# '<original args>' | <ip> <port>`.
        "mosh-client" => {
            let original = argv
                .iter()
                .position(|arg| arg == "-#")
                .and_then(|index| argv.get(index + 1))?;
            let words = original
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            return mosh_destination(&words).and_then(short_host);
        }
        _ => None,
    }?;
    short_host(destination)
}

fn ssh_destination(args: &[String]) -> Option<&str> {
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "--" {
            return args.get(index + 1).map(String::as_str);
        }
        let Some(flags) = arg.strip_prefix('-').filter(|flags| !flags.is_empty()) else {
            return Some(arg);
        };
        // Bundled flags: the first value-taking flag consumes the rest of
        // this argument, or the next argument when it ends the bundle.
        let mut takes_next = false;
        for (offset, flag) in flags.char_indices() {
            if SSH_VALUE_FLAGS.contains(&flag) {
                takes_next = offset + flag.len_utf8() == flags.len();
                break;
            }
        }
        index += if takes_next { 2 } else { 1 };
    }
    None
}

fn mosh_destination(args: &[String]) -> Option<&str> {
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "--" {
            return args.get(index + 1).map(String::as_str);
        }
        if !arg.starts_with('-') {
            return Some(arg);
        }
        index += if MOSH_VALUE_FLAGS.contains(&arg.as_str()) {
            2
        } else {
            1
        };
    }
    None
}

/// `jobs@ax42.tailf266ac.ts.net` -> `ax42`; an IP address stays whole.
fn short_host(destination: &str) -> Option<String> {
    let destination = destination.strip_prefix("ssh://").unwrap_or(destination);
    let host = destination
        .rsplit_once('@')
        .map_or(destination, |(_, host)| host);
    let host = host
        .strip_prefix('[')
        .and_then(|bracketed| bracketed.split_once(']'))
        .map_or(host, |(inner, _)| inner);
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Some(host.to_owned());
    }
    let host = host.split(':').next().unwrap_or(host);
    let label = host.split('.').next().unwrap_or(host);
    (!label.is_empty()).then(|| label.to_owned())
}

type MachineCache = HashMap<(u32, u32), Option<String>>;

/// Memoize a lookup per (pane shell pid, foreground process group). The
/// pair only changes when the pane's foreground job does, so a snapshot
/// reads process arguments once per job rather than on every publish.
pub(crate) fn cached(
    shell_pid: u32,
    process_group_id: u32,
    lookup: impl FnOnce() -> Option<String>,
) -> Option<String> {
    static CACHE: OnceLock<Mutex<MachineCache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = (shell_pid, process_group_id);
    if let Some(hit) = cache.lock().ok().and_then(|cache| cache.get(&key).cloned()) {
        return hit;
    }
    let value = lookup();
    if let Ok(mut cache) = cache.lock() {
        if cache.len() >= 4096 {
            cache.clear();
        }
        cache.insert(key, value.clone());
    }
    value
}

#[cfg(test)]
mod tests {
    use super::from_argv;

    fn machine(argv: &[&str]) -> Option<String> {
        let argv = argv.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        from_argv(argv.first().map_or("", |arg| arg.as_str()), &argv)
    }

    #[test]
    fn ssh_destinations_reduce_to_a_short_host() {
        for (argv, expected) in [
            (&["ssh", "ax42"][..], Some("ax42")),
            (&["ssh", "-t", "ax42", ".local/bin/herdr"][..], Some("ax42")),
            (
                &["/usr/bin/ssh", "jobs@ax42.tailf266ac.ts.net"][..],
                Some("ax42"),
            ),
            (
                &["ssh", "-o", "BatchMode=yes", "-p", "2222", "book"][..],
                Some("book"),
            ),
            (
                &["ssh", "-oBatchMode=yes", "-p2222", "book"][..],
                Some("book"),
            ),
            (&["ssh", "-tA", "-i", "/k", "forge-2"][..], Some("forge-2")),
            (&["ssh", "-At", "ax42", "-i", "/k"][..], Some("ax42")),
            (&["ssh", "-tl", "jobs", "ax42"][..], Some("ax42")),
            (&["ssh", "--", "pc"][..], Some("pc")),
            (&["ssh", "ssh://jobs@ax42:2222"][..], Some("ax42")),
            (
                &["ssh", "aneyman@100.107.202.126"][..],
                Some("100.107.202.126"),
            ),
            (&["ssh", "[fd7a::1]"][..], Some("fd7a::1")),
            (&["ssh", "-V"][..], None),
            (&["ssh"][..], None),
        ] {
            assert_eq!(machine(argv).as_deref(), expected, "{argv:?}");
        }
    }

    #[test]
    fn mosh_destinations_skip_option_values() {
        for (argv, expected) in [
            (&["mosh", "book"][..], Some("book")),
            (
                &["mosh", "--ssh", "ssh -p 2222", "jobs@ax42", "--", "tmux"][..],
                Some("ax42"),
            ),
            (
                &["mosh", "--ssh=ssh -p 2222", "ax42.tailf266ac.ts.net"][..],
                Some("ax42"),
            ),
            (&["mosh", "--no-init", "--", "pc"][..], Some("pc")),
            (
                &[
                    "mosh-client",
                    "-#",
                    "jobs@ax42 --no-init",
                    "|",
                    "100.64.0.9",
                    "60001",
                ][..],
                Some("ax42"),
            ),
            (&["mosh-client", "100.64.0.9", "60001"][..], None),
        ] {
            assert_eq!(machine(argv).as_deref(), expected, "{argv:?}");
        }
    }

    #[test]
    fn local_programs_have_no_machine() {
        for argv in [
            &["claude", "--resume", "ax42"][..],
            &["zsh"][..],
            &["sshd", "ax42"][..],
            &["codex", "ssh", "ax42"][..],
        ] {
            assert_eq!(machine(argv), None, "{argv:?}");
        }
    }
}
