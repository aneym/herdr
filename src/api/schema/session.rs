use serde::{Deserialize, Serialize};

use super::agents::AgentInfo;
use super::panes::{PaneInfo, PaneLayoutSnapshot};
use super::tabs::TabInfo;
use super::workspaces::WorkspaceInfo;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionSnapshot {
    pub version: String,
    pub protocol: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_pane_id: Option<String>,
    pub workspaces: Vec<WorkspaceInfo>,
    pub tabs: Vec<TabInfo>,
    pub panes: Vec<PaneInfo>,
    pub layouts: Vec<PaneLayoutSnapshot>,
    pub agents: Vec<AgentInfo>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pending_agent_restores: u32,
    /// Named sections inside Pinned, in order; each pinned tab names its own in `TabInfo.pin_section`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pin_sections: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automations: Vec<super::automations::AutomationSummary>,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}
