//! Factory overlay document (fork feature, 2026-09-28).
//!
//! An external tool (herdr-control's sync) writes one JSON file that tags tabs
//! (orchestrator, lane, workflow, advisor), nests workflow tabs under a parent tab,
//! carries a host badge and phase per tab, rolls attention up per space, and holds
//! ready-to-draw detail panels. The server polls the file (`[ui.factory] overlay_file`)
//! and ships the parsed document to clients as an endpoint control payload; the client
//! groups the sidebar and draws the detail panel from it when `[ui.factory] enabled`.
//!
//! Every field is optional with a serde default so older or newer writers still parse.
//! Unknown enum values decode to `Unknown`/`Normal`/`None` rather than failing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The only document version this build understands.
pub const FACTORY_OVERLAY_VERSION: u32 = 1;

/// Panel key for the factory overview (opened with `toggle_factory_overview`).
#[allow(dead_code)]
pub const OVERVIEW_PANEL_KEY: &str = "overview";

/// Panel key for a tab's detail panel.
#[allow(dead_code)]
pub fn tab_panel_key(tab_id: &str) -> String {
    format!("tab:{tab_id}")
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FactoryOverlay {
    pub version: u32,
    /// RFC 3339 timestamp from the writer; display only.
    pub generated_at: Option<String>,
    /// Keyed by herdr tab id, e.g. "w5H:t9E".
    pub tabs: BTreeMap<String, TabTag>,
    /// Host rows in the factory sidebar footer.
    pub hosts: Vec<HostRow>,
    /// Provider usage rows in the factory sidebar footer.
    pub usage: Vec<HostRow>,
    /// Keyed by herdr workspace id, e.g. "w5H".
    pub spaces: BTreeMap<String, SpaceTag>,
    /// Keyed by panel key: "overview" or "tab:<tab id>".
    pub panels: BTreeMap<String, Panel>,
    /// Named sidebar groups of whole spaces (e.g. Rails, Open Factory), in
    /// display order. Read from `space_groups` in areas.json beside the overlay.
    pub space_groups: Vec<SpaceGroup>,
}

/// One sidebar group. `spaces` names member workspaces by label or id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpaceGroup {
    pub name: String,
    pub spaces: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabKind {
    Orchestrator,
    Lane,
    Workflow,
    Advisor,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TabMode {
    #[default]
    Active,
    Parked,
    Auto,
}

impl<'de> Deserialize<'de> for TabMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "parked" => Self::Parked,
            "auto" => Self::Auto,
            _ => Self::Active,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attention {
    /// Amber: something is slow or degraded.
    Warn,
    /// Red: a row wants the user (failed twice, quarantined, stalled, an ask).
    Act,
    #[default]
    #[serde(other)]
    None,
}

impl Attention {
    /// Higher wants the user more; use to pick the worst of several.
    #[allow(dead_code)]
    pub fn rank(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Warn => 1,
            Self::Act => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TabSection {
    Orchestrator,
    Scoping,
    Implementing,
    Reviewing,
    Monitoring,
    Closed,
}

fn deserialize_section<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<TabSection>, D::Error> {
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(match value.as_deref() {
        Some("orchestrator") => Some(TabSection::Orchestrator),
        Some("scoping") => Some(TabSection::Scoping),
        Some("implementing" | "inflight" | "idle") => Some(TabSection::Implementing),
        Some("reviewing" | "waiting" | "ready" | "ready_for_review") => Some(TabSection::Reviewing),
        Some("monitoring") => Some(TabSection::Monitoring),
        Some("closed") => Some(TabSection::Closed),
        _ => None,
    })
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TabTag {
    pub kind: TabKind,
    pub mode: TabMode,
    pub goal: Option<String>,
    pub goal_area: Option<String>,
    #[serde(default, deserialize_with = "deserialize_section")]
    pub section: Option<TabSection>,
    /// Review page for a lane awaiting review.
    pub review_url: Option<String>,
    /// Canonical scoping doc for a lane in scoping.
    pub scope_url: Option<String>,
    /// Tab id this tab nests under (a workflow under its lane or the orchestrator).
    pub parent: Option<String>,
    /// Plain-words display name; falls back to the tab label.
    pub name: Option<String>,
    /// Short right-aligned badge, e.g. the host "PC", "Studio", "forge-2".
    pub badge: Option<String>,
    /// Short phase text, e.g. "review 3/5" or "fix 1".
    pub phase: Option<String>,
    /// Start of this workflow, in Unix epoch seconds.
    pub started: Option<i64>,
    /// Summary shown when the row is collapsed, e.g. "2 wf" or "inbox 3".
    pub summary: Option<String>,
    pub attention: Attention,
    /// Legacy writer hint; the client derives lane idle from live status instead.
    pub idle: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_reason: Option<String>,
    /// A lane or orchestrator whose seats are running; draws as working.
    pub busy: bool,
    /// Registered running workflows without their own herdr tab.
    pub runs: Vec<RunTag>,
    /// Mark a lane running the local development loop.
    pub devloop: bool,
    /// A finished tab moves to the background group.
    pub done: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunTag {
    pub id: String,
    pub name: Option<String>,
    pub phase: Option<String>,
    pub agents: u32,
    pub started: Option<i64>,
    pub done: bool,
    pub attention: Attention,
    pub badge: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HostRow {
    pub name: String,
    pub summary: Option<String>,
    pub attention: Attention,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpaceTag {
    /// Worst attention of any tagged tab in the space.
    pub attention: Attention,
    /// Tab id that wants the user; the space jump focuses it.
    pub target_tab: Option<String>,
    /// Short summary for a collapsed space header, e.g. "1".
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Panel {
    pub title: String,
    pub subtitle: Option<String>,
    pub sections: Vec<PanelSection>,
    /// Buttons drawn at the bottom, e.g. "open full orchestrator".
    pub actions: Vec<PanelRow>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelSection {
    pub title: String,
    /// Right-aligned text on the section title line, e.g. a count.
    pub right: Option<String>,
    pub rows: Vec<PanelRow>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowStyle {
    Dim,
    Accent,
    Ok,
    Warn,
    Act,
    #[default]
    #[serde(other)]
    Normal,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelRow {
    pub text: String,
    /// Right-aligned text, e.g. a host badge or an age.
    pub right: Option<String>,
    pub style: RowStyle,
    /// Nesting depth under the previous row (0 = top level).
    pub indent: u8,
    /// A herdr pane id ("w5H:p80") or tab id ("w5H:t9E"); Enter or a click focuses it.
    pub target: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OverlayParseError {
    Json(String),
    Version(u32),
}

impl std::fmt::Display for OverlayParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(err) => write!(f, "factory overlay is not valid JSON: {err}"),
            Self::Version(version) => write!(
                f,
                "factory overlay version {version} is not supported (expected {FACTORY_OVERLAY_VERSION})"
            ),
        }
    }
}

/// Parse an overlay document. Rejects malformed JSON and unknown versions only.
pub fn parse(bytes: &[u8]) -> Result<FactoryOverlay, OverlayParseError> {
    let overlay: FactoryOverlay =
        serde_json::from_slice(bytes).map_err(|err| OverlayParseError::Json(err.to_string()))?;
    if overlay.version != FACTORY_OVERLAY_VERSION {
        return Err(OverlayParseError::Version(overlay.version));
    }
    Ok(overlay)
}

/// The part of areas.json the sidebar reads; the rest belongs to Areas mode.
#[derive(Deserialize)]
struct AreasSpaceGroups {
    #[serde(default)]
    space_groups: Vec<SpaceGroup>,
}

impl FactoryOverlay {
    /// Take the space groups from an areas.json document.
    pub fn apply_areas_file(&mut self, bytes: &[u8]) -> Result<(), serde_json::Error> {
        let doc: AreasSpaceGroups = serde_json::from_slice(bytes)?;
        self.space_groups = doc
            .space_groups
            .into_iter()
            .filter(|group| !group.name.trim().is_empty())
            .collect();
        Ok(())
    }

    /// The group a space belongs to, matched by workspace id or label. The
    /// first group naming the space wins.
    pub fn space_group(&self, workspace_id: &str, label: &str) -> Option<usize> {
        let label = label.trim();
        self.space_groups.iter().position(|group| {
            group.spaces.iter().any(|name| {
                let name = name.trim();
                name == workspace_id || (!label.is_empty() && name.eq_ignore_ascii_case(label))
            })
        })
    }

    #[allow(dead_code)]
    pub fn tab(&self, tab_id: &str) -> Option<&TabTag> {
        self.tabs.get(tab_id)
    }

    #[allow(dead_code)]
    pub fn space(&self, workspace_id: &str) -> Option<&SpaceTag> {
        self.spaces.get(workspace_id)
    }

    #[allow(dead_code)]
    pub fn panel(&self, key: &str) -> Option<&Panel> {
        self.panels.get(key)
    }

    /// True when any tab of `tab_ids` carries a known kind. Spaces without tagged tabs
    /// draw exactly as they do with the overlay off.
    #[allow(dead_code)]
    pub fn space_is_tagged<'a>(&self, mut tab_ids: impl Iterator<Item = &'a str>) -> bool {
        tab_ids.any(|tab_id| {
            self.tabs
                .get(tab_id)
                .is_some_and(|tag| tag.kind != TabKind::Unknown)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_sections_accept_current_and_legacy_writer_values() {
        for (value, expected) in [
            ("orchestrator", Some(TabSection::Orchestrator)),
            ("scoping", Some(TabSection::Scoping)),
            ("implementing", Some(TabSection::Implementing)),
            ("reviewing", Some(TabSection::Reviewing)),
            ("ready", Some(TabSection::Reviewing)),
            ("ready_for_review", Some(TabSection::Reviewing)),
            ("monitoring", Some(TabSection::Monitoring)),
            ("closed", Some(TabSection::Closed)),
            ("inflight", Some(TabSection::Implementing)),
            ("waiting", Some(TabSection::Reviewing)),
            ("idle", Some(TabSection::Implementing)),
            ("bogus", None),
        ] {
            let json = format!(
                r#"{{"version":1,"tabs":{{"lane":{{"kind":"lane","section":"{value}"}}}}}}"#
            );
            let overlay = parse(json.as_bytes()).unwrap();
            assert_eq!(overlay.tabs["lane"].section, expected, "{value}");
        }
    }

    #[test]
    fn parses_a_minimal_document() {
        let overlay = parse(br#"{"version":1}"#).unwrap();
        assert!(overlay.tabs.is_empty());
        assert!(overlay.hosts.is_empty());
        assert!(overlay.usage.is_empty());
        assert!(overlay.panels.is_empty());
        let legacy = parse(br#"{"version":1,"tabs":{"lane":{"kind":"lane"}}}"#).unwrap();
        assert!(!legacy.tabs["lane"].devloop);
        assert!(legacy.tabs["lane"].runs.is_empty());
        let legacy_run = parse(
            br#"{"version":1,"tabs":{"lane":{"kind":"lane","runs":[{"id":"old","agents":2}]}}}"#,
        )
        .unwrap();
        let run = &legacy_run.tabs["lane"].runs[0];
        assert_eq!(run.id, "old");
        assert_eq!(run.agents, 2);
        assert_eq!(run.started, None);
        assert!(!run.done);
        assert_eq!(run.attention, Attention::None);
        assert_eq!(run.badge, None);
        assert_eq!(legacy.tabs["lane"].mode, TabMode::Active);
        let modes =
            parse(br#"{"version":1,"tabs":{"p":{"mode":"parked"},"x":{"mode":"weird"}}}"#).unwrap();
        assert_eq!(modes.tabs["p"].mode, TabMode::Parked);
        assert_eq!(modes.tabs["x"].mode, TabMode::Active);
        assert!(legacy.hosts.is_empty());
    }

    #[test]
    fn parses_tags_panels_and_tolerates_unknown_values() {
        let overlay = parse(
            r#"{
              "version": 1,
              "generated_at": "2026-09-28T15:40:00Z",
              "tabs": {
                "w5H:t1": {"kind": "orchestrator", "summary": "inbox 3"},
                "w5H:t2": {"kind": "lane", "name": "recruiter", "summary": "2 wf", "idle": true, "review_url": "https://rails.so/review", "scope_url": "https://rails.so/scope"},
                "w5H:t3": {"kind": "workflow", "parent": "w5H:t2", "badge": "PC",
                           "phase": "review 3/5", "attention": "act"},
                "w5H:t4": {"kind": "something-new", "attention": "purple", "extra": 1}
              },
              "spaces": {"w5H": {"attention": "act", "target_tab": "w5H:t3", "summary": "1"}},
              "panels": {
                "overview": {"title": "Factory overview", "sections": [
                  {"title": "agent-rails · Recruiter", "right": "2 tasks", "rows": [
                    {"text": "Fix outreach drafts", "right": "PC", "style": "warn",
                     "indent": 1, "target": "w5H:p9"},
                    {"text": "odd", "style": "blink"}
                  ]}
                ], "actions": [{"text": "open full orchestrator", "target": "w5H:t1"}]}
              }
            }"#
            .as_bytes(),
        )
        .unwrap();
        assert_eq!(overlay.tab("w5H:t1").unwrap().kind, TabKind::Orchestrator);
        let workflow = overlay.tab("w5H:t3").unwrap();
        assert_eq!(workflow.parent.as_deref(), Some("w5H:t2"));
        assert_eq!(workflow.badge.as_deref(), Some("PC"));
        assert_eq!(workflow.attention, Attention::Act);
        assert!(overlay.tab("w5H:t2").unwrap().idle);
        let serialized = serde_json::to_value(overlay.tab("w5H:t2").unwrap()).unwrap();
        assert_eq!(serialized["review_url"], "https://rails.so/review");
        assert_eq!(serialized["scope_url"], "https://rails.so/scope");
        let absent = serde_json::to_value(overlay.tab("w5H:t1").unwrap()).unwrap();
        assert!(absent["review_url"].is_null());
        assert!(absent["scope_url"].is_null());
        let unknown = overlay.tab("w5H:t4").unwrap();
        assert_eq!(unknown.kind, TabKind::Unknown);
        assert_eq!(unknown.attention, Attention::None);
        assert_eq!(overlay.space("w5H").unwrap().attention, Attention::Act);
        let panel = overlay.panel(OVERVIEW_PANEL_KEY).unwrap();
        assert_eq!(panel.sections[0].rows[0].style, RowStyle::Warn);
        assert_eq!(panel.sections[0].rows[1].style, RowStyle::Normal);
        assert_eq!(panel.actions[0].target.as_deref(), Some("w5H:t1"));
        assert!(overlay.space_is_tagged(["w5H:t9", "w5H:t2"].into_iter()));
        assert!(!overlay.space_is_tagged(["w5H:t4", "w5H:t9"].into_iter()));
        assert_eq!(tab_panel_key("w5H:t2"), "tab:w5H:t2");
    }

    #[test]
    fn hosts_and_devloop_round_trip() {
        let json = br#"{"version":1,"tabs":{"lane":{"kind":"lane","devloop":true,"goal":"rails","goal_area":"workspace ui"}},"hosts":[{"name":"PC","summary":"3/28 live","attention":"warn"}],"usage":[{"name":"claude","summary":"3/8 - 26%","url":"https://studio.tailf266ac.ts.net:2455/"}]}"#;
        let parsed = parse(json).unwrap();
        assert!(parsed.tabs["lane"].devloop);
        assert_eq!(
            serde_json::to_value(&parsed.tabs["lane"]).unwrap()["goal"],
            "rails"
        );
        assert_eq!(
            serde_json::to_value(&parsed.tabs["lane"]).unwrap()["goal_area"],
            "workspace ui"
        );
        assert_eq!(parsed.hosts[0].summary.as_deref(), Some("3/28 live"));
        assert_eq!(parsed.hosts[0].attention, Attention::Warn);
        assert_eq!(parsed.hosts[0].url, None);
        assert_eq!(parsed.usage[0].summary.as_deref(), Some("3/8 - 26%"));
        assert_eq!(
            parsed.usage[0].url.as_deref(),
            Some("https://studio.tailf266ac.ts.net:2455/")
        );
        let encoded = serde_json::to_vec(&parsed).unwrap();
        assert_eq!(parse(&encoded).unwrap(), parsed);
    }

    #[test]
    fn rejects_bad_json_and_unknown_versions() {
        assert!(matches!(parse(b"{"), Err(OverlayParseError::Json(_))));
        assert_eq!(
            parse(br#"{"version":2}"#),
            Err(OverlayParseError::Version(2))
        );
        assert_eq!(parse(br#"{}"#), Err(OverlayParseError::Version(0)));
    }
}
