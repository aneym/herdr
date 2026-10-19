use crate::api::schema::*;
use crate::app::pin_sections::PinSectionError;
use crate::app::App;

use super::responses::{encode_error, encode_success};

fn section_error(id: String, error: PinSectionError) -> String {
    encode_error(id, error.code(), error.message())
}

impl App {
    fn pin_sections_changed(&mut self, ws: Option<usize>) {
        self.state.mark_session_dirty();
        self.schedule_session_save();
        let ws = ws.or(self.state.active).unwrap_or(0);
        if ws < self.state.workspaces.len() {
            let workspace = self.workspace_info(ws);
            self.emit_event(EventEnvelope {
                event: EventKind::WorkspaceUpdated,
                data: EventData::WorkspaceUpdated { workspace },
            });
        }
    }

    pub(super) fn pin_sections_result(&self, id: String) -> String {
        let sections = self
            .state
            .pin_sections
            .iter()
            .map(|name| PinSectionInfo {
                name: name.clone(),
                tab_ids: self.state.pin_section_members(name),
            })
            .collect();
        let unsectioned = self
            .state
            .pinned_tabs
            .iter()
            .filter(|pin| pin.role.is_none() && pin.section.is_none())
            .map(|pin| pin.tab_id.clone())
            .collect();
        encode_success(
            id,
            ResponseResult::PinSections {
                sections,
                unsectioned,
            },
        )
    }

    pub(super) fn handle_pin_section_create(
        &mut self,
        id: String,
        params: PinSectionCreateParams,
    ) -> String {
        match self
            .state
            .create_pin_section(&params.name, params.after.as_deref())
        {
            Ok(_) => {
                self.pin_sections_changed(None);
                self.pin_sections_result(id)
            }
            Err(error) => section_error(id, error),
        }
    }

    pub(super) fn handle_pin_section_rename(
        &mut self,
        id: String,
        params: PinSectionRenameParams,
    ) -> String {
        match self
            .state
            .rename_pin_section(&params.name, &params.new_name)
        {
            Ok(new_name) => {
                if params.name.trim() != new_name {
                    self.journal_pin_event(
                        "section_rename",
                        params.source.as_deref(),
                        serde_json::json!({"old": params.name.trim(), "new": new_name}),
                    );
                }
                self.pin_sections_changed(None);
                self.pin_sections_result(id)
            }
            Err(error) => section_error(id, error),
        }
    }

    pub(super) fn handle_pin_section_delete(
        &mut self,
        id: String,
        params: PinSectionTarget,
    ) -> String {
        let tab_ids = self.state.pin_section_members(params.name.trim());
        match self.state.delete_pin_section(&params.name) {
            Ok(()) => {
                self.journal_pin_event(
                    "section_delete",
                    params.source.as_deref(),
                    serde_json::json!({"name": params.name.trim(), "tab_ids": tab_ids}),
                );
                self.pin_sections_changed(None);
                self.pin_sections_result(id)
            }
            Err(error) => section_error(id, error),
        }
    }

    pub(super) fn handle_pin_section_move(
        &mut self,
        id: String,
        params: PinSectionMoveParams,
    ) -> String {
        match self.state.move_pin_section(&params.name, params.index) {
            Ok(_) => {
                self.pin_sections_changed(None);
                self.pin_sections_result(id)
            }
            Err(error) => section_error(id, error),
        }
    }

    pub(super) fn handle_tab_set_pin_section(
        &mut self,
        id: String,
        params: TabSetPinSectionParams,
    ) -> String {
        let Some((ws_idx, tab_idx)) = self.parse_tab_id(&params.tab_id) else {
            return encode_error(id, "tab_not_found", "tab not found");
        };
        let Some(tab_id) = self.public_tab_id(ws_idx, tab_idx) else {
            return encode_error(id, "tab_not_found", "tab not found");
        };
        let before = self.state.pinned_tabs.clone();
        if let Err(error) = self
            .state
            .set_pin_section(&tab_id, params.section.as_deref())
        {
            return section_error(id, error);
        }
        if self.state.pinned_tabs != before {
            self.journal_pin_change("section", params.source.as_deref(), &tab_id);
            self.pin_sections_changed(Some(ws_idx));
        }
        match self.tab_info(ws_idx, tab_idx) {
            Some(tab) => encode_success(id, ResponseResult::TabInfo { tab }),
            None => encode_error(id, "tab_not_found", "tab not found"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn request(app: &mut App, method: &str, params: Value) -> Value {
        let request =
            serde_json::from_value(json!({"id":"sec", "method":method, "params":params})).unwrap();
        serde_json::from_str(&app.handle_api_request(request)).unwrap()
    }

    #[test]
    fn pin_sections_api_creates_places_and_reports_sections() {
        let (_, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            rx,
            crate::api::EventHub::default(),
        );
        app.state.workspaces = vec![crate::workspace::Workspace::test_new("pins")];
        app.state.workspaces[0].id = "w1".into();
        app.state.workspaces[0].test_add_tab(Some("b"));
        app.state.workspaces[0].test_add_tab(Some("c"));
        app.state.active = Some(0);
        app.state.ensure_test_terminals();
        let tabs: Vec<String> = (0..3).map(|i| app.public_tab_id(0, i).unwrap()).collect();

        for name in ["Needs you", "Later"] {
            assert_eq!(
                request(&mut app, "pin_section.create", json!({"name":name}))["result"]["type"],
                "pin_sections"
            );
        }
        let created = request(
            &mut app,
            "pin_section.create",
            json!({"name":"Northstar: building", "after":"Needs you"}),
        );
        assert_eq!(
            created["result"]["sections"][1]["name"],
            "Northstar: building"
        );
        assert_eq!(
            request(&mut app, "pin_section.create", json!({"name":"Later"}))["error"]["code"],
            "pin_section_exists"
        );

        let before = app.event_hub.current_sequence();
        let pinned = request(
            &mut app,
            "tab.set_pinned",
            json!({"tab_id":tabs[0], "pinned":true, "section":"Later"}),
        );
        assert_eq!(pinned["result"]["tab"]["pin_section"], "Later");
        assert!(!app.event_hub.events_after(before).is_empty());
        request(
            &mut app,
            "tab.set_pinned",
            json!({"tab_id":tabs[1], "pinned":true}),
        );
        let placed = request(
            &mut app,
            "tab.set_pin_section",
            json!({"tab_id":tabs[1], "section":"Needs you"}),
        );
        assert_eq!(placed["result"]["tab"]["pin_section"], "Needs you");
        request(
            &mut app,
            "tab.set_pinned",
            json!({"tab_id":tabs[2], "pinned":true}),
        );
        assert_eq!(
            request(
                &mut app,
                "tab.set_pinned",
                json!({"tab_id":tabs[2], "pinned":true, "section":"Nope"})
            )["error"]["code"],
            "pin_section_not_found"
        );

        let listed = request(&mut app, "pin_section.list", json!({}));
        assert_eq!(listed["result"]["unsectioned"], json!([tabs[2]]));
        assert_eq!(listed["result"]["sections"][0]["tab_ids"], json!([tabs[1]]));
        assert_eq!(listed["result"]["sections"][2]["tab_ids"], json!([tabs[0]]));
        let order: Vec<_> = app
            .state
            .pinned_tabs
            .iter()
            .map(|p| p.tab_id.clone())
            .collect();
        assert_eq!(order, [tabs[2].clone(), tabs[1].clone(), tabs[0].clone()]);
        assert_eq!(app.tab_info(0, 2).unwrap().pin_index, Some(0));

        let snapshot: Value = serde_json::from_str(
            &app.handle_api_request(
                serde_json::from_value(json!({"id":"s","method":"session.snapshot","params":{}}))
                    .unwrap(),
            ),
        )
        .unwrap();
        assert_eq!(
            snapshot["result"]["snapshot"]["pin_sections"],
            json!(["Needs you", "Northstar: building", "Later"])
        );

        request(
            &mut app,
            "pin_section.move",
            json!({"name":"Later", "index":0}),
        );
        request(
            &mut app,
            "pin_section.rename",
            json!({"name":"Later", "new_name":"Someday"}),
        );
        assert_eq!(
            app.tab_info(0, 0).unwrap().pin_section.as_deref(),
            Some("Someday")
        );
        let deleted = request(&mut app, "pin_section.delete", json!({"name":"Someday"}));
        assert_eq!(deleted["result"]["unsectioned"], json!([tabs[2], tabs[0]]));
        assert!(app.tab_info(0, 0).unwrap().pin_section.is_none());
        let cleared = request(
            &mut app,
            "tab.set_pin_section",
            json!({"tab_id":tabs[1], "section":null}),
        );
        assert!(cleared["result"]["tab"].get("pin_section").is_none());
        crate::app::api::test_support::shutdown_test_runtimes(&mut app);
    }
}
