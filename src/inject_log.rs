//! Content-free audit trail of text accepted for injection into a pane.
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::Mutex;

use sha2::{Digest, Sha256};
use time::{format_description, OffsetDateTime};

// Shared across API callers so only the first successful write per directory and UTC day prunes.
static LAST_PRUNED_DAY: Mutex<Option<(std::path::PathBuf, time::Date)>> = Mutex::new(None);

pub fn record(pane_id: &str, method: &str, text: &str) {
    let dir = std::env::var_os("HERDR_INJECT_LOG_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::session::data_dir().join("inject-log"));
    if let Err(err) = write_record(&dir, pane_id, method, text) {
        // Do not include text, pane ids, paths, or OS error details in diagnostic logs.
        tracing::warn!(error_kind = ?err.kind(), "could not record pane injection");
    }
}

fn write_record(dir: &Path, pane_id: &str, method: &str, text: &str) -> io::Result<()> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Ok(());
    }
    let now = OffsetDateTime::now_utc();
    let date = now.date();
    let date_name = date.to_string();
    let stamp_format = format_description::parse(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z",
    )
    .map_err(io::Error::other)?;
    let stamp = now.format(&stamp_format).map_err(io::Error::other)?;
    let digest = format!("{:x}", Sha256::digest(normalized.as_bytes()));
    let line = serde_json::json!({
        "ts": stamp,
        "pane_id": pane_id,
        "method": method,
        "sha256": digest,
        "len": normalized.chars().count(),
    })
    .to_string()
        + "\n";

    create_private_dir(dir)?;
    let path = dir.join(format!("{date_name}.jsonl"));
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    // write_all can issue several writes; one append syscall keeps records intact across processes.
    if file.write(line.as_bytes())? != line.len() {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "short injection log write",
        ));
    }

    let mut last = LAST_PRUNED_DAY
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if last
        .as_ref()
        .is_none_or(|(previous_dir, previous_day)| previous_dir != dir || *previous_day != date)
    {
        prune(dir, date)?;
        *last = Some((dir.to_path_buf(), date));
    }
    Ok(())
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700).create(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(dir)?;
    Ok(())
}

fn prune(dir: &Path, today: time::Date) -> io::Result<()> {
    let cutoff = (today - time::Duration::days(14)).to_string();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(stem) = name.strip_suffix(".jsonl") else {
            continue;
        };
        // Accept only canonical YYYY-MM-DD names, not arbitrary paths or suffixed files.
        if stem.len() != 10
            || !stem.as_bytes().iter().enumerate().all(|(i, c)| {
                if i == 4 || i == 7 {
                    *c == b'-'
                } else {
                    c.is_ascii_digit()
                }
            })
        {
            continue;
        }
        if stem < cutoff.as_str() && entry.file_type()?.is_file() {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub(crate) static TEST_ENV_LOCK: Mutex<()> = Mutex::new(());
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

    fn test_dir() -> std::path::PathBuf {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target/inject-log-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn normalized_hash_and_content_free_json() {
        let dir = test_dir();
        write_record(&dir, "w1:p2", "pane.send_input", "  hello \n world ").unwrap();
        let path = dir.join(format!("{}.jsonl", OffsetDateTime::now_utc().date()));
        let contents = fs::read_to_string(&path).unwrap();
        let row: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
        assert_eq!(
            row["sha256"],
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        assert_eq!(row["len"], 11);
        assert_eq!(row["method"], "pane.send_input");
        assert_eq!(row["pane_id"], "w1:p2");
        assert_eq!(row.as_object().unwrap().len(), 5);
        assert!(!contents.contains("hello"));
        assert!(row["ts"].as_str().unwrap().ends_with('Z'));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn retention_uses_filename_date() {
        let dir = test_dir();
        let today = OffsetDateTime::now_utc().date();
        let old = dir.join(format!("{}.jsonl", today - time::Duration::days(15)));
        let recent = dir.join(format!("{}.jsonl", today - time::Duration::days(13)));
        fs::write(&old, "old").unwrap();
        fs::write(&recent, "recent").unwrap();
        prune(&dir, today).unwrap();
        assert!(!old.exists());
        assert!(recent.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unwritable_destination_does_not_panic() {
        let _guard = TEST_ENV_LOCK.lock().unwrap();
        // A regular file cannot be created as a directory even for root.
        let dir = test_dir();
        let file = dir.join("not-a-directory");
        fs::write(&file, "").unwrap();
        let previous = std::env::var_os("HERDR_INJECT_LOG_DIR");
        std::env::set_var("HERDR_INJECT_LOG_DIR", &file);
        record("w1:p2", "agent.prompt", "hello");
        if let Some(previous) = previous {
            std::env::set_var("HERDR_INJECT_LOG_DIR", previous);
        } else {
            std::env::remove_var("HERDR_INJECT_LOG_DIR");
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
