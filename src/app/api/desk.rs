use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;

use crate::api::schema::*;
use crate::app::App;

use super::responses::{encode_error, encode_success};

const MAX_FILE_SIZE: u64 = 20 * 1024 * 1024;

impl App {
    fn desk_target(&self, target: &DeskTarget) -> Result<(usize, String), &'static str> {
        let (ws, tab) = if let Some(id) = &target.tab_id {
            self.parse_tab_id(id).ok_or("tab_not_found")?
        } else if let Some(id) = &target.pane_id {
            let (ws, pane) = self.parse_pane_id(id).ok_or("pane_not_found")?;
            let tab = self.state.workspaces[ws]
                .find_tab_index_for_pane(pane)
                .ok_or("pane_not_found")?;
            (ws, tab)
        } else {
            return Err("desk_target_required");
        };
        Ok((ws, self.public_tab_id(ws, tab).ok_or("tab_not_found")?))
    }

    fn tab_desk(&self, ws: usize, tab_id: String) -> TabDesk {
        let desk = self
            .state
            .desks
            .get(&tab_id)
            .map(|d| d.info.clone())
            .unwrap_or_default();
        TabDesk {
            workspace_id: self.public_workspace_id(ws),
            tab_id,
            desk,
        }
    }

    fn desk_mutated(&mut self, id: String, ws: usize, tab_id: String, item_id: String) -> String {
        self.state.mark_session_dirty();
        self.schedule_session_save();
        let desk = self.tab_desk(ws, tab_id);
        self.emit_event(EventEnvelope {
            event: EventKind::DeskChanged,
            data: EventData::DeskChanged {
                workspace_id: desk.workspace_id.clone(),
                tab_id: desk.tab_id.clone(),
                desk: desk.desk.clone(),
            },
        });
        encode_success(id, ResponseResult::Desk { desk, item_id })
    }

    pub(super) fn handle_desk_open(&mut self, id: String, params: DeskOpenParams) -> String {
        let (ws, tab_id) = match self.desk_target(&params.target) {
            Ok(target) => target,
            Err(code) => return encode_error(id, code, "desk target is unavailable"),
        };
        let opened_by = params.opened_by.unwrap_or_else(|| "api".into());
        if opened_by.chars().count() > 64 {
            return encode_error(
                id,
                "desk_invalid_opened_by",
                "opened_by exceeds 64 characters",
            );
        }
        let reference = params.reference;
        let (kind, title, mime) = if let Some(rest) = reference
            .strip_prefix("http://")
            .or_else(|| reference.strip_prefix("https://"))
        {
            let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
            if authority.is_empty()
                || reference
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control())
            {
                return encode_error(
                    id,
                    "desk_invalid_ref",
                    "expected an http(s) URL with a host",
                );
            }
            let host = authority.rsplit('@').next().unwrap_or(authority);
            if host.is_empty() || host.starts_with(':') {
                return encode_error(id, "desk_invalid_ref", "URL host is missing");
            }
            let path = rest[authority.len()..]
                .split(['?', '#'])
                .next()
                .unwrap_or_default();
            let title = format!("{host}{path}");
            (DeskItemKind::Url, title, "text/html".into())
        } else {
            let path = Path::new(&reference);
            if !path.is_absolute() {
                return encode_error(
                    id,
                    "desk_invalid_ref",
                    "expected an absolute file path or http(s) URL",
                );
            }
            if !path.is_file() {
                return encode_error(
                    id,
                    "desk_file_not_found",
                    "file does not exist on the server",
                );
            }
            let title = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            (DeskItemKind::File, title, file_mime(path).into())
        };
        let item = DeskItem {
            id: String::new(),
            kind,
            reference,
            title: params.title.unwrap_or(title),
            mime,
            opened_by,
            opened_at_ms: time_ms(SystemTime::now()),
        };
        let Some(item_id) = self.state.desk_open(&tab_id, item, params.background) else {
            return encode_error(id, "desk_id_exhausted", "desk item ids exhausted");
        };
        self.desk_mutated(id, ws, tab_id, item_id)
    }

    pub(super) fn handle_desk_close(&mut self, id: String, params: DeskCloseParams) -> String {
        let (ws, tab_id) = match self.desk_target(&params.target) {
            Ok(target) => target,
            Err(code) => return encode_error(id, code, "desk target is unavailable"),
        };
        let Some(item_id) = self.state.desk_close(&tab_id, params.item.as_deref()) else {
            return encode_error(id, "desk_item_not_found", "desk item not found");
        };
        self.desk_mutated(id, ws, tab_id, item_id)
    }

    pub(super) fn handle_desk_focus(&mut self, id: String, params: DeskFocusParams) -> String {
        let (ws, tab_id) = match self.desk_target(&params.target) {
            Ok(target) => target,
            Err(code) => return encode_error(id, code, "desk target is unavailable"),
        };
        let Some(item_id) = self.state.desk_focus(&tab_id, &params.item) else {
            return encode_error(id, "desk_item_not_found", "desk item not found");
        };
        self.desk_mutated(id, ws, tab_id, item_id)
    }

    pub(super) fn handle_desk_list(&mut self, id: String, target: DeskTarget) -> String {
        let desks = if target.tab_id.is_some() || target.pane_id.is_some() {
            match self.desk_target(&target) {
                Ok((ws, tab_id)) => vec![self.tab_desk(ws, tab_id)],
                Err(code) => return encode_error(id, code, "desk target is unavailable"),
            }
        } else {
            let mut desks = Vec::new();
            for (ws, workspace) in self.state.workspaces.iter().enumerate() {
                for tab in 0..workspace.tabs.len() {
                    if let Some(tab_id) = self.public_tab_id(ws, tab) {
                        if self
                            .state
                            .desks
                            .get(&tab_id)
                            .is_some_and(|d| !d.info.items.is_empty())
                        {
                            desks.push(self.tab_desk(ws, tab_id));
                        }
                    }
                }
            }
            desks
        };
        encode_success(id, ResponseResult::DeskList { desks })
    }

    pub(super) fn handle_desk_read(&mut self, id: String, params: DeskReadParams) -> String {
        let (_, tab_id) = match self.desk_target(&params.target) {
            Ok(target) => target,
            Err(code) => return encode_error(id, code, "desk target is unavailable"),
        };
        let Some(item) = self.state.desks.get(&tab_id).and_then(|desk| {
            desk.info
                .items
                .iter()
                .find(|i| i.id == params.item || i.reference == params.item)
        }) else {
            return encode_error(id, "desk_item_not_found", "desk item not found");
        };
        if item.kind != DeskItemKind::File {
            return encode_error(id, "desk_not_a_file", "desk item is not a file");
        }
        let read = || -> Result<ResponseResult, (&str, String)> {
            let metadata = std::fs::metadata(&item.reference)
                .map_err(|e| ("desk_file_not_found", e.to_string()))?;
            if !metadata.is_file() {
                return Err(("desk_not_a_file", "path is no longer a regular file".into()));
            }
            if metadata.len() > MAX_FILE_SIZE {
                return Err(("desk_file_too_large", "file exceeds 20 MB".into()));
            }
            let mtime_ms = time_ms(
                metadata
                    .modified()
                    .map_err(|e| ("desk_read_failed", e.to_string()))?,
            );
            let unchanged = params.known_mtime_ms == Some(mtime_ms);
            let mut size = metadata.len();
            let data_base64 = if unchanged {
                None
            } else {
                let file = std::fs::File::open(&item.reference)
                    .map_err(|e| ("desk_read_failed", e.to_string()))?;
                let mut bytes = Vec::new();
                file.take(MAX_FILE_SIZE + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| ("desk_read_failed", e.to_string()))?;
                size = bytes.len() as u64;
                if size > MAX_FILE_SIZE {
                    return Err(("desk_file_too_large", "file exceeds 20 MB".into()));
                }
                Some(base64::engine::general_purpose::STANDARD.encode(bytes))
            };
            Ok(ResponseResult::DeskFile {
                item_id: item.id.clone(),
                mime: item.mime.clone(),
                size,
                mtime_ms,
                unchanged,
                data_base64,
            })
        };
        match read() {
            Ok(result) => encode_success(id, result),
            Err((code, message)) => encode_error(id, code, message),
        }
    }
}

fn time_ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn file_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "md" | "markdown" => "text/markdown",
        "html" | "htm" => "text/html",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "text/plain",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn request(app: &mut App, method: &str, params: Value) -> Value {
        let before = app.event_hub.current_sequence();
        let request =
            serde_json::from_value(json!({"id":"desk", "method":method, "params":params})).unwrap();
        let response: Value = serde_json::from_str(&app.handle_api_request(request)).unwrap();
        if method.starts_with("desk.") {
            let events = app.event_hub.events_after(before);
            let mutates = matches!(method, "desk.open" | "desk.close" | "desk.focus")
                && response.get("error").is_none();
            assert_eq!(events.len(), usize::from(mutates), "{method}: {response}");
            if mutates {
                assert_eq!(events[0].1.event, EventKind::DeskChanged);
                let wire = serde_json::to_value(&events[0].1).unwrap();
                assert_eq!(wire["event"], "desk.changed");
                assert_eq!(wire["data"]["tab_id"], response["result"]["desk"]["tab_id"]);
                assert_eq!(
                    wire["data"]["desk"]["front"],
                    response["result"]["desk"]["front"]
                );
            }
        }
        response
    }

    /// Public API ownership: duplicate/background/front transitions, file bytes,
    /// target resolution and events. The real tempfile boundary catches read/mtime
    /// regressions; there was no desk API coverage and no production test seam.
    #[test]
    fn desk_api_round_trip() {
        let events = crate::api::EventHub::default();
        let (_, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            rx,
            events.clone(),
        );
        app.state.workspaces = vec![crate::workspace::Workspace::test_new("desk")];
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
        let temp = std::env::temp_dir().join(format!(
            "herdr-desk-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&temp).unwrap();
        let file = temp.join("doc.md");
        std::fs::write(&file, "# Desk\n").unwrap();
        let path = file.to_str().unwrap();
        let url = request(
            &mut app,
            "desk.open",
            json!({"tab_id":tab, "ref":"https://example.com/doc"}),
        );
        assert_eq!(url["result"]["item_id"], "d1");
        assert_eq!(
            url["result"]["desk"]["items"][0]["title"],
            "example.com/doc"
        );
        let md = request(&mut app, "desk.open", json!({"tab_id":tab, "ref":path}));
        assert_eq!(md["result"]["item_id"], "d2");
        assert_eq!(md["result"]["desk"]["front"], "d2");
        let duplicate = request(
            &mut app,
            "desk.open",
            json!({"tab_id":tab, "ref":"https://example.com/doc"}),
        );
        assert_eq!(duplicate["result"]["desk"]["front"], "d1");
        assert_eq!(
            duplicate["result"]["desk"]["items"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let background = request(
            &mut app,
            "desk.open",
            json!({"tab_id":tab, "ref":"https://example.com/background", "background":true}),
        );
        assert_eq!(background["result"]["desk"]["front"], "d1");
        let duplicate_background = request(
            &mut app,
            "desk.open",
            json!({"tab_id":tab,"ref":path,"background":true}),
        );
        assert_eq!(duplicate_background["result"]["desk"]["front"], "d1");
        assert_eq!(
            duplicate_background["result"]["desk"]["items"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        let read = request(&mut app, "desk.read", json!({"tab_id":tab, "item":"d2"}));
        assert_eq!(read["result"]["mime"], "text/markdown");
        assert_eq!(read["result"]["size"], 7);
        assert_eq!(read["result"]["data_base64"], "IyBEZXNrCg==");
        let unchanged = request(
            &mut app,
            "desk.read",
            json!({"tab_id":tab, "item":path, "known_mtime_ms":read["result"]["mtime_ms"]}),
        );
        assert_eq!(unchanged["result"]["unchanged"], true);
        assert!(unchanged["result"]["data_base64"].is_null());
        assert_eq!(
            request(&mut app, "desk.read", json!({"tab_id":tab,"item":"d1"}))["error"]["code"],
            "desk_not_a_file"
        );
        assert_eq!(
            request(&mut app, "desk.close", json!({"tab_id":tab}))["result"]["desk"]["front"],
            "d2"
        );
        assert_eq!(
            request(&mut app, "desk.focus", json!({"tab_id":tab,"item":"d3"}))["result"]["desk"]
                ["front"],
            "d3"
        );
        assert_eq!(
            request(&mut app, "desk.close", json!({"tab_id":tab}))["result"]["desk"]["front"],
            "d2"
        );
        assert_eq!(
            request(&mut app, "desk.focus", json!({"tab_id":tab,"item":path}))["result"]["item_id"],
            "d2"
        );
        let via_pane = request(
            &mut app,
            "desk.open",
            json!({"pane_id":pane_id,"ref":"https://example.com/pane"}),
        );
        assert_eq!(via_pane["result"]["desk"]["tab_id"], other);
        assert_eq!(
            request(&mut app, "desk.open", json!({"ref":"https://example.com"}))["error"]["code"],
            "desk_target_required"
        );
        assert_eq!(
            request(
                &mut app,
                "desk.open",
                json!({"tab_id":tab,"ref":"relative.md"})
            )["error"]["code"],
            "desk_invalid_ref"
        );
        assert_eq!(
            request(
                &mut app,
                "desk.focus",
                json!({"tab_id":tab,"item":"missing"})
            )["error"]["code"],
            "desk_item_not_found"
        );
        assert_eq!(
            request(
                &mut app,
                "desk.open",
                json!({"tab_id":tab,"ref":"ftp://example.com"})
            )["error"]["code"],
            "desk_invalid_ref"
        );
        assert_eq!(
            request(
                &mut app,
                "desk.open",
                json!({"tab_id":tab,"ref":temp.join("missing.md")})
            )["error"]["code"],
            "desk_file_not_found"
        );
        assert_eq!(
            request(
                &mut app,
                "desk.open",
                json!({"tab_id":tab,"ref":"https://example.com", "opened_by":"a".repeat(65)})
            )["error"]["code"],
            "desk_invalid_opened_by"
        );
        assert_eq!(
            request(
                &mut app,
                "desk.list",
                json!({"tab_id":tab,"pane_id":"not-a-pane"})
            )["result"]["desks"][0]["tab_id"],
            tab
        );
        assert_eq!(
            request(&mut app, "desk.list", json!({}))["result"]["desks"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            app.tab_info(0, 0).unwrap().desk.unwrap().front.as_deref(),
            Some("d2")
        );
        // Closing through the actual tab API must prune the desk immediately.
        request(&mut app, "tab.close", json!({"tab_id":other}));
        assert!(!app.state.desks.contains_key(&other));
        // Allocation counters survive clearing; capacity preserves the front.
        request(&mut app, "desk.close", json!({"tab_id":tab}));
        assert!(app.tab_info(0, 0).unwrap().desk.is_none());
        assert_eq!(
            request(
                &mut app,
                "desk.open",
                json!({"tab_id":tab,"ref":"https://example.com/next"})
            )["result"]["item_id"],
            "d4"
        );
        for n in 0..33 {
            request(
                &mut app,
                "desk.open",
                json!({"tab_id":tab,"ref":format!("https://example.com/{n}"),"background":true}),
            );
        }
        let desk = &app.state.desks[&tab].info;
        assert_eq!(desk.items.len(), 32);
        assert_eq!(desk.front.as_deref(), Some("d4"));
        assert!(!desk.items.iter().any(|i| i.id == "d5"));
        let oversized = temp.join("large.bin");
        std::fs::File::create(&oversized)
            .unwrap()
            .set_len(MAX_FILE_SIZE + 1)
            .unwrap();
        let opened = request(&mut app, "desk.open", json!({"tab_id":tab,"ref":oversized}));
        assert_eq!(
            request(
                &mut app,
                "desk.read",
                json!({"tab_id":tab,"item":opened["result"]["item_id"]})
            )["error"]["code"],
            "desk_file_too_large"
        );
        app.state.assert_invariants_for_test();
        crate::app::api::test_support::shutdown_test_runtimes(&mut app);
        std::fs::remove_dir_all(temp).unwrap();
    }
}
