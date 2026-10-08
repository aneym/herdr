#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HomeLocation {
    Cloud,
    Local,
    Unsynced,
}

/// Ignore future home classifications in snapshots from newer endpoints.
pub fn deserialize_home_location<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<HomeLocation>, D::Error> {
    if !deserializer.is_human_readable() {
        return Option::<HomeLocation>::deserialize(deserializer);
    }
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(match value.as_deref() {
        Some("cloud") => Some(HomeLocation::Cloud),
        Some("local") => Some(HomeLocation::Local),
        Some("unsynced") => Some(HomeLocation::Unsynced),
        _ => None,
    })
}

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::common::AgentStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TabRole {
    Agent,
}

/// Snapshots tolerate roles introduced by newer servers; API input stays strict.
pub fn deserialize_pin_role<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<TabRole>, D::Error> {
    if !deserializer.is_human_readable() {
        return Option::<TabRole>::deserialize(deserializer);
    }
    let role = Option::<String>::deserialize(deserializer)?;
    Ok(role
        .filter(|value| value == "agent")
        .map(|_| TabRole::Agent))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TabSetRoleParams {
    pub tab_id: String,
    #[serde(default)]
    pub role: Option<TabRole>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TabSetHiddenParams {
    pub tab_id: String,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TabCreateParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub focus: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct TabListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TabRenameParams {
    pub tab_id: String,
    pub label: String,
}

/// Pin or unpin a chat in the sidebar's pinned section. `priority` sets where
/// it lands in pin order (higher first); omitted keeps/appends at the end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TabSetPinnedParams {
    pub tab_id: String,
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i64>,
}

/// Move a pinned chat to `pin_index` in the shared pin order (0 is the top,
/// the Cmd+1 slot), the numbering `TabInfo.pin_index` reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TabPinMoveParams {
    pub tab_id: String,
    pub pin_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TabMoveParams {
    pub tab_id: String,
    pub insert_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TabInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_location: Option<HomeLocation>,
    #[serde(default)]
    pub sort_rank: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desk: Option<super::desk::DeskInfo>,
    pub tab_id: String,
    pub workspace_id: String,
    pub number: usize,
    pub label: String,
    pub focused: bool,
    pub pane_count: usize,
    pub agent_status: AgentStatus,
    /// Whether this chat is working, by the one rule every surface draws
    /// (`app/work_status.rs`): its agent panes' statuses, working while it owns a live factory run. Absent on servers that predate it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_status: Option<AgentStatus>,
    /// Position in the pinned-chats order, when pinned (Cmd+1..9 slot).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<TabRole>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

/// Classify strict role decoding errors consistently at both JSON front doors.
pub(crate) fn invalid_role_request(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .is_some_and(|value| {
            value["method"] == "tab.set_role"
                && value["params"]
                    .get("role")
                    .is_some_and(|role| !role.is_null() && role != "agent")
        })
}
