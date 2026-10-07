//! Slice R2 scenario, resolver: an agent's home location read from a real
//! cards directory (spec agents-hide-and-home-glyph-2026-10-07, "Server
//! resolution of home location"). Integration at the file boundary: every
//! case writes the same card, marker and status files agent-rails and
//! `bin/agent-home` write, and reads the poller's map, never a private parser.
//!
//! Cards resolve as Mac `AgentCards.load` resolves them: folders sorted by
//! name, `name` and `pane` non-empty, pane not `"none"`, and the first card
//! per pane wins (frank before health, as on the Studio today).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::AgentHomePoller;
use crate::api::schema::HomeLocation;

const FRANK_PANE: &str = "w5Y:p1";
const CONTENT_PANE: &str = "w5Y:p2";
const HOME_PANE: &str = "w5Y:p3";
const RECRUITER_PANE: &str = "w5Y:p4";
const GARBLED_PANE: &str = "w5Y:p5";

/// A throwaway `HERDR_AGENTS_DIR`, passed to the poller explicitly.
struct CardsDir(PathBuf);

impl CardsDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "herdr-agent-home-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, contents).unwrap();
        path
    }

    /// `<folder>/agent.json` as agent-rails writes it.
    fn card(&self, folder: &str, name: &str, pane: &str) -> PathBuf {
        self.write(
            &format!("{folder}/agent.json"),
            &serde_json::json!({
                "name": name,
                "plugin_id": format!("aneym.{name}"),
                "branch": "main",
                "pane": pane,
                "check": "sh ./check",
            })
            .to_string(),
        )
    }

    /// `<folder>/.rails/home.json`: the folder has a Rails cloud home.
    fn marker(&self, folder: &str) -> PathBuf {
        self.write(
            &format!("{folder}/.rails/home.json"),
            r#"{"origin":"https://rails.so","agent_id":"agt_1","revision":7,"head":"4b825dc"}"#,
        )
    }

    /// `<folder>/.rails/status.json` in the schema `bin/agent-home` writes.
    fn status(&self, folder: &str, location: &str) -> PathBuf {
        let unsaved = u32::from(location != "cloud");
        self.write(
            &format!("{folder}/.rails/status.json"),
            &serde_json::json!({
                "schema": 1,
                "location": location,
                "unsaved": unsaved,
                "unsent": 0,
                "revision": 7,
                "rails_revision": 7,
                "error": null,
                "checked_at": "2026-10-07T18:00:00Z",
            })
            .to_string(),
        )
    }

    /// Every file under the dir, for pinning mtimes.
    fn files(&self) -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push(path);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.0, &mut out);
        out
    }
}

impl Drop for CardsDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Pins a file's mtime well in the past. A poller that also re-reads files
/// written in the last few seconds (as the factory overlay poller does, for
/// coarse mtime resolution) would otherwise mask what an unchanged poll reads.
fn set_mtime(path: &Path, at: SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

/// The five cards on the Studio today, plus the edge cases the card and
/// status rules name: a card with pane `"none"`, a garbled status file, and a
/// stray top-level file that is no card folder.
fn studio_cards(tag: &str) -> CardsDir {
    let dir = CardsDir::new(tag);
    // Sorts first; a pane of "none" names no row even with a cloud home.
    dir.card("aaa-parked", "parked", "none");
    dir.marker("aaa-parked");
    dir.status("aaa-parked", "cloud");
    // Marker and a clean status: cloud.
    dir.card("frank", "frank", FRANK_PANE);
    dir.marker("frank");
    dir.status("frank", "cloud");
    // No .rails at all: local.
    dir.card("content", "content", CONTENT_PANE);
    // Marker but no status yet: unsynced.
    dir.card("home", "home", HOME_PANE);
    dir.marker("home");
    // Marker and a status that reports unsynced.
    dir.card("recruiter", "recruiter", RECRUITER_PANE);
    dir.marker("recruiter");
    dir.status("recruiter", "unsynced");
    // Same pane as frank and sorts after it; local if it ever won.
    dir.card("health", "health", FRANK_PANE);
    // Marker and an unreadable (half-written) status: unsynced.
    dir.card("zz-garbled", "garbled", GARBLED_PANE);
    dir.marker("zz-garbled");
    dir.write("zz-garbled/.rails/status.json", r#"{"schema":1,"locat"#);
    dir.write("README.md", "not a card folder");
    let settled = SystemTime::now() - Duration::from_secs(3600);
    for file in dir.files() {
        set_mtime(&file, settled);
    }
    dir
}

fn expected_studio_map() -> HashMap<String, HomeLocation> {
    HashMap::from([
        (FRANK_PANE.to_owned(), HomeLocation::Cloud),
        (CONTENT_PANE.to_owned(), HomeLocation::Local),
        (HOME_PANE.to_owned(), HomeLocation::Unsynced),
        (RECRUITER_PANE.to_owned(), HomeLocation::Unsynced),
        (GARBLED_PANE.to_owned(), HomeLocation::Unsynced),
    ])
}

/// Protects the home-location definitions (local, cloud, unsynced) and the
/// card rules shared with the Mac Shell. Fails if a missing or garbled status
/// reads as cloud or local, if a `"none"` pane enters the map, or if the last
/// card per pane wins instead of the first (health stealing frank's pane).
#[test]
fn home_location_resolver_classifies_cards_and_first_card_per_pane_wins() {
    let dir = studio_cards("classify");
    let mut poller = AgentHomePoller::default();

    assert!(poller.poll(dir.path()), "the first poll fills an empty map");
    assert_eq!(poller.homes(), &expected_studio_map());
}

/// Protects the cost contract (one readdir and stats per poll, re-reading only
/// files whose mtime or length changed) and freshness (a status edit shows on
/// the next poll). Fails if an unchanged poll re-reads files or reports a
/// change, if an edited status file is not picked up, or if a later card on a
/// claimed pane can move that pane's location.
#[test]
fn home_location_poll_rereads_only_changed_files_and_flips_on_a_status_edit() {
    let dir = studio_cards("poll");
    let mut poller = AgentHomePoller::default();
    assert!(poller.poll(dir.path()));
    let after_first = poller.file_reads();
    assert!(after_first > 0, "the first poll reads the cards it maps");

    // Nothing on disk changed: no change, and not one file read.
    assert!(!poller.poll(dir.path()));
    assert_eq!(poller.file_reads(), after_first);
    assert_eq!(poller.homes(), &expected_studio_map());

    // agent-home finds unsaved files and rewrites frank's status.
    let status = dir.status("frank", "unsynced");
    set_mtime(&status, SystemTime::now() - Duration::from_secs(1800));
    assert!(
        poller.poll(dir.path()),
        "the status edit flips the next poll"
    );
    assert_eq!(
        poller.file_reads(),
        after_first + 1,
        "only the edited status file is re-read"
    );
    let mut expected = expected_studio_map();
    expected.insert(FRANK_PANE.to_owned(), HomeLocation::Unsynced);
    assert_eq!(poller.homes(), &expected);

    // health, on frank's pane, gains a clean cloud home. frank still owns the
    // pane, so its unsynced location stands and nothing changed.
    for path in [dir.marker("health"), dir.status("health", "cloud")] {
        set_mtime(&path, SystemTime::now() - Duration::from_secs(1700));
    }
    assert!(!poller.poll(dir.path()));
    assert_eq!(poller.homes(), &expected);

    // The next sync leaves frank clean again.
    let status = dir.status("frank", "cloud");
    set_mtime(&status, SystemTime::now() - Duration::from_secs(1600));
    assert!(poller.poll(dir.path()));
    assert_eq!(poller.homes(), &expected_studio_map());
}
