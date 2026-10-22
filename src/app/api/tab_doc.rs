use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::schema::*;
use crate::app::App;

use super::responses::{encode_error, encode_success};

impl App {
    fn tab_target(&self, target: &TabDocTarget) -> Result<(usize, String), &'static str> {
        let (ws, tab) = if let Some(id) = &target.tab_id {
            self.parse_tab_id(id).ok_or("tab_not_found")?
        } else if let Some(id) = &target.pane_id {
            let (ws, pane) = self.parse_pane_id(id).ok_or("pane_not_found")?;
            let tab = self.state.workspaces[ws]
                .find_tab_index_for_pane(pane)
                .ok_or("pane_not_found")?;
            (ws, tab)
        } else {
            return Err("tab_target_required");
        };
        Ok((ws, self.public_tab_id(ws, tab).ok_or("tab_not_found")?))
    }

    fn tab_doc_result(&self, id: String, ws: usize, tab_id: String) -> String {
        let doc = self
            .state
            .docs
            .get(&tab_id)
            .and_then(|entry| entry.doc.clone());
        encode_success(
            id,
            ResponseResult::TabDoc {
                workspace_id: self.public_workspace_id(ws),
                tab_id,
                doc,
            },
        )
    }

    fn tab_doc_changed(&mut self, id: String, ws: usize, tab_id: String) -> String {
        self.state.mark_session_dirty();
        self.schedule_session_save();
        let workspace = self.workspace_info(ws);
        self.emit_event(EventEnvelope {
            event: EventKind::WorkspaceUpdated,
            data: EventData::WorkspaceUpdated { workspace },
        });
        self.tab_doc_result(id, ws, tab_id)
    }

    pub(super) fn handle_tab_doc_set(&mut self, id: String, params: TabDocSetParams) -> String {
        let (ws, tab_id) = match self.tab_target(&params.target) {
            Ok(target) => target,
            Err(code) => return encode_error(id, code, "tab target is unavailable"),
        };
        let url = params.url.trim().to_owned();
        let default_title = match doc_default_title(&url) {
            Ok(title) => title,
            Err(message) => return encode_error(id, "tab_doc_invalid_url", message),
        };
        let title = params
            .title
            .map(|title| title.trim().to_owned())
            .filter(|title| !title.is_empty())
            .unwrap_or(default_title);
        if title.chars().count() > 200 {
            return encode_error(id, "tab_doc_invalid_title", "title exceeds 200 characters");
        }
        if params
            .set_by
            .as_ref()
            .is_some_and(|by| by.chars().count() > 64)
        {
            return encode_error(id, "tab_doc_invalid_set_by", "set_by exceeds 64 characters");
        }
        let doc = TabDoc {
            url,
            title,
            set_by: params.set_by,
            set_at_ms: time_ms(SystemTime::now()),
        };
        self.state.docs.entry(tab_id.clone()).or_default().doc = Some(doc);
        self.tab_doc_changed(id, ws, tab_id)
    }

    pub(super) fn handle_tab_doc_clear(&mut self, id: String, target: TabDocTarget) -> String {
        let (ws, tab_id) = match self.tab_target(&target) {
            Ok(target) => target,
            Err(code) => return encode_error(id, code, "tab target is unavailable"),
        };
        match self.state.docs.get_mut(&tab_id) {
            Some(entry) if entry.doc.is_some() => {
                entry.doc = None;
                self.tab_doc_changed(id, ws, tab_id)
            }
            _ => self.tab_doc_result(id, ws, tab_id),
        }
    }

    pub(super) fn handle_tab_doc_get(&mut self, id: String, target: TabDocTarget) -> String {
        match self.tab_target(&target) {
            Ok((ws, tab_id)) => self.tab_doc_result(id, ws, tab_id),
            Err(code) => encode_error(id, code, "tab target is unavailable"),
        }
    }
}

fn doc_default_title(url: &str) -> Result<String, &'static str> {
    if url.is_empty() || url.len() > 4096 || url.chars().any(char::is_control) {
        return Err("expected an http(s) URL or absolute path");
    }
    if let Some(rest) = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
    {
        if url.chars().any(char::is_whitespace) {
            return Err("URL contains whitespace");
        }
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let host = authority.rsplit('@').next().unwrap_or(authority);
        if host.is_empty() || host.starts_with(':') {
            return Err("URL host is missing");
        }
        let path = rest[authority.len()..]
            .split(['?', '#'])
            .next()
            .unwrap_or_default()
            .trim_end_matches('/');
        return Ok(format!("{host}{path}"));
    }
    let path = Path::new(url);
    if !path.is_absolute() {
        return Err("expected an http(s) URL or absolute path");
    }
    Ok(path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned())
}

fn time_ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn request(app: &mut App, method: &str, params: Value) -> Value {
        let request =
            serde_json::from_value(json!({"id":"doc", "method":method, "params":params})).unwrap();
        serde_json::from_str(&app.handle_api_request(request)).unwrap()
    }

    #[test]
    fn tab_doc_set_get_clear_by_pane_and_tab() {
        let events = crate::api::EventHub::default();
        let (_, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            rx,
            events.clone(),
        );
        app.state.workspaces = vec![crate::workspace::Workspace::test_new("doc")];
        app.state.workspaces[0].id = "w1".into();
        app.state.workspaces[0].test_add_tab(Some("other"));
        app.state.active = Some(0);
        app.state.ensure_test_terminals();
        let tab = app.public_tab_id(0, 0).unwrap();
        let other = app.public_tab_id(0, 1).unwrap();
        let pane = app.state.workspaces[0].tabs[1]
            .panes
            .keys()
            .next()
            .copied()
            .unwrap();
        let pane_id = app.public_pane_id(0, pane).unwrap();

        let before = app.event_hub.current_sequence();
        let set = request(
            &mut app,
            "tab.doc_set",
            json!({"pane_id":pane_id, "url":"https://studio.example/scope/plan/", "set_by":"pane:x"}),
        );
        assert_eq!(set["result"]["type"], "tab_doc");
        assert_eq!(set["result"]["tab_id"], other);
        assert_eq!(
            set["result"]["doc"]["url"],
            "https://studio.example/scope/plan/"
        );
        assert_eq!(set["result"]["doc"]["title"], "studio.example/scope/plan");
        assert_eq!(set["result"]["doc"]["set_by"], "pane:x");
        let emitted = app.event_hub.events_after(before);
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].1.event, EventKind::WorkspaceUpdated);

        let titled = request(
            &mut app,
            "tab.doc_set",
            json!({"tab_id":other, "url":"https://studio.example/b", "title":"Rails scope"}),
        );
        assert_eq!(titled["result"]["doc"]["title"], "Rails scope");
        let info = app.tab_info(0, 1).unwrap();
        assert_eq!(
            info.doc.as_ref().map(|d| d.url.as_str()),
            Some("https://studio.example/b")
        );
        assert!(app.tab_info(0, 0).unwrap().doc.is_none());
        let wire = serde_json::to_value(app.tab_info(0, 0).unwrap()).unwrap();
        assert!(wire.get("doc").is_none());

        let got = request(&mut app, "tab.doc_get", json!({"tab_id":other}));
        assert_eq!(got["result"]["doc"]["title"], "Rails scope");
        let file = request(
            &mut app,
            "tab.doc_set",
            json!({"tab_id":tab, "url":"/tmp/My Plan.md"}),
        );
        assert_eq!(file["result"]["doc"]["title"], "My Plan.md");

        for (params, code) in [
            (
                json!({"tab_id":tab, "url":"relative/doc.md"}),
                "tab_doc_invalid_url",
            ),
            (
                json!({"tab_id":tab, "url":"https:///nohost"}),
                "tab_doc_invalid_url",
            ),
            (
                json!({"tab_id":tab, "url":"https://a.example/x y"}),
                "tab_doc_invalid_url",
            ),
            (
                json!({"tab_id":tab, "url":"https://a.example", "title":"t".repeat(201)}),
                "tab_doc_invalid_title",
            ),
            (
                json!({"tab_id":"w1:t99", "url":"https://a.example"}),
                "tab_not_found",
            ),
            (json!({"url":"https://a.example"}), "tab_target_required"),
        ] {
            assert_eq!(
                request(&mut app, "tab.doc_set", params)["error"]["code"],
                code
            );
        }
        assert_eq!(
            app.tab_info(0, 0).unwrap().doc.map(|d| d.url),
            Some("/tmp/My Plan.md".to_string())
        );

        let cleared = request(&mut app, "tab.doc_clear", json!({"pane_id":pane_id}));
        assert!(cleared["result"]["doc"].is_null());
        assert!(app.tab_info(0, 1).unwrap().doc.is_none());
        let before = app.event_hub.current_sequence();
        let again = request(&mut app, "tab.doc_clear", json!({"tab_id":other}));
        assert!(again["result"]["doc"].is_null());
        assert!(app.event_hub.events_after(before).is_empty());
    }
}
