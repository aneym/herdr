use crate::api::schema::{AutomationSummary, EventData, EventEnvelope, EventKind};
use crate::app::App;

impl App {
    pub(crate) fn set_automations(&mut self, automations: Vec<AutomationSummary>) {
        self.state.automations = automations;
        for index in 0..self.state.workspaces.len() {
            let workspace = self.workspace_info(index);
            self.emit_event(EventEnvelope {
                event: EventKind::WorkspaceUpdated,
                data: EventData::WorkspaceUpdated { workspace },
            });
        }
    }
}
