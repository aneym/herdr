use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeskItemKind {
    Url,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeskItem {
    pub id: String,
    pub kind: DeskItemKind,
    #[serde(rename = "ref")]
    pub reference: String,
    pub title: String,
    pub mime: String,
    pub opened_by: String,
    pub opened_at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeskInfo {
    pub items: Vec<DeskItem>,
    pub front: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TabDesk {
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(flatten)]
    pub desk: DeskInfo,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeskTarget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeskOpenParams {
    #[serde(flatten)]
    pub target: DeskTarget,
    #[serde(rename = "ref")]
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened_by: Option<String>,
    #[serde(default)]
    pub background: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeskCloseParams {
    #[serde(flatten)]
    pub target: DeskTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeskFocusParams {
    #[serde(flatten)]
    pub target: DeskTarget,
    pub item: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeskReadParams {
    #[serde(flatten)]
    pub target: DeskTarget,
    pub item: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_mtime_ms: Option<u64>,
}
