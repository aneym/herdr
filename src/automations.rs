use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::api::schema::AutomationSummary;

pub(crate) fn automations_dir() -> PathBuf {
    std::env::var_os("HERDR_AUTOMATIONS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::worktree::expand_tilde_path("~/.agent-rails/factory/automations"))
}

fn text(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

fn parse(folder: &str, bytes: &[u8]) -> Option<AutomationSummary> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let report_url = text(&value, "report_url").filter(|url| {
        url.get(..8)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
    });
    Some(AutomationSummary {
        id: text(&value, "id").unwrap_or_else(|| folder.to_owned()),
        name: text(&value, "name")?,
        cadence: text(&value, "cadence"),
        status: text(&value, "status"),
        result: text(&value, "result"),
        last_run: text(&value, "last_run"),
        next_run: text(&value, "next_run"),
        report: text(&value, "report"),
        report_url,
    })
}

#[derive(Default)]
pub(crate) struct AutomationsPoller {
    stamps: HashMap<PathBuf, (Option<SystemTime>, u64)>,
    parsed: HashMap<PathBuf, Option<AutomationSummary>>,
    summaries: Vec<AutomationSummary>,
}

impl AutomationsPoller {
    pub(crate) fn poll(&mut self, dir: &Path) -> bool {
        let mut folders: Vec<_> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .collect();
        folders.sort_by_key(|entry| entry.file_name());
        let mut summaries = Vec::new();
        let mut seen = Vec::new();
        for entry in folders {
            let path = entry.path().join("latest.json");
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            let stamp = (meta.modified().ok(), meta.len());
            if self.stamps.get(&path) != Some(&stamp) {
                let folder = entry.file_name().to_string_lossy().into_owned();
                let parsed = std::fs::read(&path)
                    .ok()
                    .and_then(|bytes| parse(&folder, &bytes));
                self.stamps.insert(path.clone(), stamp);
                self.parsed.insert(path.clone(), parsed);
            }
            if let Some(Some(summary)) = self.parsed.get(&path) {
                summaries.push(summary.clone());
            }
            seen.push(path);
        }
        self.stamps.retain(|path, _| seen.contains(path));
        self.parsed.retain(|path, _| seen.contains(path));
        if summaries == self.summaries {
            return false;
        }
        self.summaries = summaries;
        true
    }

    pub(crate) fn summaries(&self) -> &[AutomationSummary] {
        &self.summaries
    }
}
