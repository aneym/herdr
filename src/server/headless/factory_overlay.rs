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
pub(crate) struct FactoryOverlayPoller {
    last_seen: Option<FileStamp>,
    last_areas: Option<FileStamp>,
    /// Space groups from the last areas.json that parsed. A malformed or
    /// half-written areas file keeps these instead of dropping the groups.
    last_space_groups: Option<Vec<crate::factory_overlay::SpaceGroup>>,
    pub(crate) revision: u64,
    pub(crate) current: Option<Arc<FactoryOverlay>>,
    /// mtime of the last overlay that parsed. A malformed rewrite keeps the
    /// old overlay and must not make it look freshly written.
    current_written: Option<SystemTime>,
    logged_errors: HashSet<String>,
}

impl FactoryOverlayPoller {
    pub(crate) fn poll(&mut self, path: Option<&Path>) -> Option<Option<Arc<FactoryOverlay>>> {
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
        let areas_path = path.with_file_name("areas.json");
        let areas_stamp = fs::metadata(&areas_path).ok().map(|metadata| FileStamp {
            path: areas_path.clone(),
            modified: metadata.modified().ok(),
            len: metadata.len(),
        });
        let areas_recent = areas_stamp
            .as_ref()
            .and_then(|stamp| stamp.modified)
            .is_some_and(|time| {
                SystemTime::now()
                    .duration_since(time)
                    .is_ok_and(|age| age <= Duration::from_secs(2))
            });
        let stamp_changed =
            self.last_seen.as_ref() != Some(&stamp) || self.last_areas != areas_stamp;
        if !stamp_changed && !recent && !areas_recent {
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
        self.last_areas = areas_stamp;
        match crate::factory_overlay::parse(&bytes) {
            Ok(mut overlay) => {
                self.apply_space_groups(&mut overlay, &areas_path);
                self.current_written = modified;
                self.publish(Some(Arc::new(overlay)), stamp_changed)
            }
            Err(error) => {
                self.log_error(format!("{}: {error}", path.display()));
                None
            }
        }
    }

    /// areas.json owns the space groups whenever it exists: a parsed file
    /// replaces the overlay's own groups (an empty or missing `space_groups`
    /// clears them), an unreadable or malformed one keeps the last parsed
    /// groups. Without areas.json the overlay's own `space_groups` stand.
    /// Herdr Shell's LaneCatalog applies the same rule.
    fn apply_space_groups(&mut self, overlay: &mut FactoryOverlay, areas_path: &Path) {
        match fs::read(areas_path) {
            Ok(bytes) => match overlay.apply_areas_file(&bytes) {
                Ok(()) => self.last_space_groups = Some(overlay.space_groups.clone()),
                Err(error) => {
                    self.log_error(format!("{}: {error}", areas_path.display()));
                    if let Some(groups) = &self.last_space_groups {
                        overlay.space_groups = groups.clone();
                    }
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.last_space_groups = None;
            }
            Err(error) => {
                self.log_error(format!("{}: {error}", areas_path.display()));
                if let Some(groups) = &self.last_space_groups {
                    overlay.space_groups = groups.clone();
                }
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

    /// When the current overlay file was last written, for freshness checks.
    pub(crate) fn written(&self) -> Option<SystemTime> {
        self.current.as_ref()?;
        self.current_written
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

    /// Review of bbbf052a: a malformed rewrite after a valid overlay went
    /// stale kept the old overlay but reported the failed write's mtime, so
    /// its expired workflows read working again.
    #[test]
    fn factory_overlay_poller_keeps_the_parsed_overlays_write_time_over_a_malformed_rewrite() {
        let dir = std::env::temp_dir().join(format!(
            "herdr-factory-written-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("overlay.json");
        let set_mtime = |at: SystemTime| {
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(at))
                .unwrap();
        };
        let valid_at = SystemTime::now() - Duration::from_secs(300);
        let mut poller = FactoryOverlayPoller::default();
        fs::write(
            &path,
            br#"{"version":1,"tabs":{"one":{"kind":"workflow"}}}"#,
        )
        .unwrap();
        set_mtime(valid_at);
        let valid_at = fs::metadata(&path).unwrap().modified().unwrap();
        assert!(poller.poll(Some(&path)).is_some());
        assert_eq!(poller.written(), Some(valid_at));

        fs::write(&path, b"not json").unwrap();
        set_mtime(valid_at + Duration::from_secs(121));
        assert!(poller.poll(Some(&path)).is_none());
        assert!(poller.current.is_some());
        assert_eq!(poller.written(), Some(valid_at));
        fs::remove_dir_all(&dir).unwrap();
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
    #[test]
    fn areas_file_owns_space_groups_and_a_bad_write_keeps_the_last_ones() {
        let dir = std::env::temp_dir().join(format!(
            "herdr-factory-groups-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("overlay.json");
        let areas = dir.join("areas.json");
        let names = |poller: &FactoryOverlayPoller| -> Vec<String> {
            poller.current.as_deref().map_or_else(Vec::new, |overlay| {
                overlay
                    .space_groups
                    .iter()
                    .map(|group| group.name.clone())
                    .collect()
            })
        };
        let mut poller = FactoryOverlayPoller::default();
        // No areas.json: the overlay's own groups stand.
        fs::write(
            &path,
            br#"{"version":1,"space_groups":[{"name":"Own","spaces":["a"]}]}"#,
        )
        .unwrap();
        poller.poll(Some(&path));
        assert_eq!(names(&poller), ["Own"]);

        // areas.json wins over the overlay's own groups.
        fs::write(
            &areas,
            br#"{"space_groups":[{"name":"Rails","spaces":["a"]}]}"#,
        )
        .unwrap();
        poller.poll(Some(&path));
        assert_eq!(names(&poller), ["Rails"]);

        // A half-written areas.json keeps the last parsed groups.
        fs::write(&areas, br#"{"space_groups":[{"na"#).unwrap();
        poller.poll(Some(&path));
        assert_eq!(names(&poller), ["Rails"]);

        // A parsed areas.json without groups clears them on purpose.
        fs::write(&areas, br#"{"areas":[]}"#).unwrap();
        poller.poll(Some(&path));
        assert!(names(&poller).is_empty());

        fs::remove_dir_all(&dir).unwrap();
    }
}
