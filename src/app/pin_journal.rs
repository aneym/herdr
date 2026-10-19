use std::fs::OpenOptions;
use std::io::Write;

use serde_json::{json, Value};

use super::App;

impl App {
    pub(super) fn journal_pin_change(&self, op: &str, source: Option<&str>, tab_id: &str) {
        let pin_index = self.state.pinned_tab_index(tab_id);
        let section = pin_index.and_then(|index| self.state.pinned_tabs[index].section.as_deref());
        let label = self
            .parse_tab_id(tab_id)
            .and_then(|(ws, tab)| self.tab_info(ws, tab))
            .map(|tab| tab.label);
        self.journal_pin_event(
            op,
            source,
            json!({
                "tab_id": tab_id,
                "label": label,
                "section": section,
                "pin_index": pin_index,
            }),
        );
    }

    pub(super) fn journal_pin_event(&self, op: &str, source: Option<&str>, mut event: Value) {
        let path = std::env::var_os("HERDR_PIN_JOURNAL").map(std::path::PathBuf::from);
        #[cfg(test)]
        if path.is_none() {
            return;
        }
        let path = path.unwrap_or_else(|| crate::session::data_dir().join("pin-journal.jsonl"));
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let at = time::OffsetDateTime::now_utc()
                .replace_nanosecond(0)?
                .format(&time::format_description::well_known::Rfc3339)?;
            event["at"] = json!(at);
            event["op"] = json!(op);
            event["source"] = json!(source.unwrap_or("ui"));
            for key in ["tab_id", "label", "section", "pin_index"] {
                if event.get(key).is_none() {
                    event[key] = Value::Null;
                }
            }
            event["pinned_tab_ids"] = json!(self
                .state
                .pinned_tabs
                .iter()
                .map(|pin| &pin.tab_id)
                .collect::<Vec<_>>());
            let mut line = serde_json::to_vec(&event)?;
            line.push(b'\n');
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
            file.write_all(&line)?;
            file.sync_data()?;
            Ok(())
        })();
        if let Err(error) = result {
            tracing::warn!(path = %path.display(), %error, "could not append pin journal");
        }
    }
}
