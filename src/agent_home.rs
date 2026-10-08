//! Filesystem-only agent home resolution, polled by the server scheduler.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::api::schema::HomeLocation;

fn parse_card(bytes: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let name = value.get("name")?.as_str()?;
    let pane = value.get("pane")?.as_str()?;
    (!name.is_empty() && !pane.is_empty() && pane != "none").then(|| pane.to_owned())
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
        let mut visited = HashSet::new();
        for entry in folders {
            let folder = entry.path();
            let card = folder.join("agent.json");
            let marker = folder.join(".rails/home.json");
            let status = folder.join(".rails/status.json");
            visited.extend([card.clone(), marker.clone(), status.clone()]);
            let pane = self.file(&card).bytes.as_deref().and_then(parse_card);
            let marker = self.file(&marker);
            let has_marker = marker.exists;
            let valid_marker = marker.bytes.as_deref().is_some_and(parse_marker);
            let status = self.file(&status).bytes.as_deref().map(parse_status);
            if let Some(pane) = pane {
                homes.entry(pane).or_insert_with(|| {
                    if !has_marker {
                        HomeLocation::Local
                    } else if !valid_marker {
                        HomeLocation::Unsynced
                    } else {
                        status.unwrap_or(HomeLocation::Unsynced)
                    }
                });
            }
        }
        self.files.retain(|path, _| visited.contains(path));
        if self.homes == homes {
            false
        } else {
            self.homes = homes;
            true
        }
    }

    pub(crate) fn homes(&self) -> &HashMap<String, HomeLocation> {
        &self.homes
    }

    #[cfg(test)]
    pub(crate) fn file_reads(&self) -> u64 {
        self.file_reads
    }
}

#[cfg(test)]
mod tests;
