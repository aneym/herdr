use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tracing::warn;

use crate::factory_overlay::FactoryOverlay;

#[derive(PartialEq, Eq)]
struct FileStamp {
    path: PathBuf,
    modified: Option<SystemTime>,
    len: u64,
}

#[derive(Default)]
pub(super) struct FactoryOverlayPoller {
    last_seen: Option<FileStamp>,
    pub(super) revision: u64,
    pub(super) current: Option<Arc<FactoryOverlay>>,
    logged_errors: HashSet<String>,
}

impl FactoryOverlayPoller {
    pub(super) fn poll(&mut self, path: Option<&Path>) -> Option<Option<Arc<FactoryOverlay>>> {
        let path = path?;
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.log_error(format!("{}: {error}", path.display()));
                self.last_seen = None;
                return self.publish(None, false);
            }
            Err(error) => {
                self.log_error(format!("{}: {error}", path.display()));
                return None;
            }
        };
        let modified = metadata.modified().ok();
        let stamp = FileStamp {
            path: path.to_owned(),
            modified,
            len: metadata.len(),
        };
        // A writer can replace a file twice inside the filesystem's mtime resolution.
        let recent = modified.is_none_or(|time| {
            SystemTime::now()
                .duration_since(time)
                .is_ok_and(|age| age <= Duration::from_secs(2))
        });
        let stamp_changed = self.last_seen.as_ref() != Some(&stamp);
        if !stamp_changed && !recent {
            return None;
        }
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.log_error(format!("{}: {error}", path.display()));
                return None;
            }
        };
        self.last_seen = Some(stamp);
        match crate::factory_overlay::parse(&bytes) {
            Ok(overlay) => self.publish(Some(Arc::new(overlay)), stamp_changed),
            Err(error) => {
                self.log_error(format!("{}: {error}", path.display()));
                None
            }
        }
    }

    fn publish(
        &mut self,
        overlay: Option<Arc<FactoryOverlay>>,
        stamp_changed: bool,
    ) -> Option<Option<Arc<FactoryOverlay>>> {
        if !stamp_changed && self.current.as_deref() == overlay.as_deref() {
            return None;
        }
        self.revision = self.revision.saturating_add(1);
        self.current = overlay.clone();
        Some(overlay)
    }

    fn log_error(&mut self, error: String) {
        if self.logged_errors.insert(error.clone()) {
            warn!(%error, "failed to load factory overlay");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_overlay_poller_publishes_only_valid_changes_and_removal() {
        let dir = std::env::temp_dir().join(format!(
            "herdr-factory-poll-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("overlay.json");
        let mut poller = FactoryOverlayPoller::default();
        fs::write(&path, br#"{"version":1,"tabs":{"one":{"kind":"lane"}}}"#).unwrap();
        let first = poller.poll(Some(&path)).unwrap().unwrap();
        assert!(first.tabs.contains_key("one"));
        assert_eq!(poller.revision, 1);
        assert!(poller.poll(Some(&path)).is_none());
        assert_eq!(poller.revision, 1);

        fs::write(
            &path,
            br#"{"version":1,"tabs":{"two":{"kind":"workflow"}}}"#,
        )
        .unwrap();
        let second = poller.poll(Some(&path)).unwrap().unwrap();
        assert!(second.tabs.contains_key("two"));
        assert_eq!(poller.revision, 2);

        fs::write(&path, b"not json").unwrap();
        assert!(poller.poll(Some(&path)).is_none());
        assert_eq!(poller.current.as_deref(), Some(second.as_ref()));
        assert_eq!(poller.revision, 2);

        fs::remove_file(&path).unwrap();
        assert!(matches!(poller.poll(Some(&path)), Some(None)));
        assert_eq!(poller.revision, 3);
        assert!(poller.poll(Some(&path)).is_none());
        fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn factory_overlay_poller_records_missing_file_once() {
        let path = std::env::temp_dir().join(format!(
            "herdr-factory-missing-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut poller = FactoryOverlayPoller::default();
        assert!(poller.poll(Some(&path)).is_none());
        assert!(poller.poll(Some(&path)).is_none());
        assert_eq!(poller.logged_errors.len(), 1);
        assert_eq!(poller.revision, 0);
    }
}
