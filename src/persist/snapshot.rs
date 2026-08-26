use std::collections::HashMap;
use std::path::PathBuf;

use ratatui::layout::Direction;
use serde::{Deserialize, Serialize};

use crate::layout::Node;
use crate::terminal::TerminalRuntimeRegistry;
use crate::workspace::{Workspace, DEFAULT_PROFILE};

fn default_profile() -> String {
    DEFAULT_PROFILE.to_string()
}

fn default_true() -> bool {
    true
}

fn deserialize_profile<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let profile = String::deserialize(deserializer)?;
    Ok(crate::workspace::normalize_profile_name_lossy(&profile).unwrap_or_else(default_profile))
}

fn deserialize_profiles<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let profiles = Vec::<String>::deserialize(deserializer)?;
    Ok(crate::workspace::normalize_profiles_lossy(profiles))
}

/// Current snapshot format version.
pub(super) const SNAPSHOT_VERSION: u32 = 3;

/// Serializable snapshot of the entire herdr session.
#[derive(Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Format version — used to detect incompatible changes.
    #[serde(default)]
    pub version: u32,
    pub workspaces: Vec<WorkspaceSnapshot>,
    pub active: Option<usize>,
    #[serde(default = "default_profile", deserialize_with = "deserialize_profile")]
    pub active_profile: String,
    pub selected: usize,
    #[serde(default)]
    pub sidebar_width: Option<u16>,
    #[serde(default)]
    pub sidebar_section_split: Option<f32>,
    #[serde(default)]
    pub collapsed_space_keys: std::collections::HashSet<String>,
    #[serde(default)]
    pub automations_expanded: bool,
    #[serde(default)]
    pub collapsed_agent_group_keys: std::collections::HashSet<String>,
    #[serde(default = "default_true")]
    pub tree_show_spaces: bool,
    #[serde(default = "default_true")]
    pub tree_show_tabs: bool,
    #[serde(default = "default_true")]
    pub tree_show_agents: bool,
    #[serde(default)]
    pub tree_collapsed_spaces: std::collections::HashSet<String>,
    #[serde(default)]
    pub tree_collapsed_tabs: std::collections::HashSet<String>,
    #[serde(default)]
    pub tree_pinned_spaces: std::collections::HashSet<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SessionHistorySnapshot {
    /// Format version follows the matching session snapshot version.
    #[serde(default)]
    pub version: u32,
    pub workspaces: Vec<WorkspaceHistorySnapshot>,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceHistorySnapshot {
    pub tabs: Vec<TabHistorySnapshot>,
}

#[derive(Serialize, Deserialize)]
pub struct TabHistorySnapshot {
    pub panes: HashMap<u32, PaneHistorySnapshot>,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub custom_name: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_profiles"
    )]
    pub profiles: Vec<String>,
    pub identity_cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_space: Option<crate::workspace::WorktreeSpaceMembership>,
    #[serde(default)]
    pub public_pane_numbers: HashMap<u32, usize>,
    #[serde(default)]
    pub next_public_pane_number: usize,
    #[serde(default)]
    pub public_tab_numbers: Vec<usize>,
    #[serde(default)]
    pub next_public_tab_number: usize,
    pub tabs: Vec<TabSnapshot>,
    #[serde(default)]
    pub active_tab: usize,
    #[serde(default)]
    pub orchestrator_mode: bool,
}

#[derive(Deserialize)]
struct LegacyWorkspaceSnapshot {
    #[serde(default)]
    custom_name: Option<String>,
    layout: LayoutSnapshot,
    panes: HashMap<u32, PaneSnapshot>,
    zoomed: bool,
    #[serde(default)]
    focused: Option<u32>,
    #[serde(default)]
    root_pane: Option<u32>,
}

#[derive(Serialize, Deserialize)]
pub struct TabSnapshot {
    #[serde(default)]
    pub custom_name: Option<String>,
    pub layout: LayoutSnapshot,
    pub panes: HashMap<u32, PaneSnapshot>,
    pub zoomed: bool,
    #[serde(default)]
    pub focused: Option<u32>,
    #[serde(default)]
    pub root_pane: Option<u32>,
}

#[derive(Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_agent_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<PaneAgentSessionSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_argv: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_ownership: Option<PaneAgentOwnershipSnapshot>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_profiles"
    )]
    pub profiles: Vec<String>,
    /// Last synced terminal title (agent thread titles live here), so sidebar
    /// titles survive restarts instead of waiting for agents to re-emit them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneAgentSessionSnapshot {
    pub source: String,
    pub agent: String,
    pub kind: crate::agent_resume::AgentSessionRefKind,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneAgentOwnershipSnapshot {
    pub origin: PaneAgentOwnerRefSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<PaneAgentOwnerRefSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneAgentOwnerRefSnapshot {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<PaneAgentSessionSnapshot>,
}

impl PaneAgentSessionSnapshot {
    fn from_session(session: &crate::agent_resume::PersistedAgentSession) -> Self {
        Self {
            source: session.source.clone(),
            agent: session.agent.clone(),
            kind: session.session_ref.kind,
            value: session.session_ref.value.clone(),
        }
    }

    fn to_session(&self) -> crate::agent_resume::PersistedAgentSession {
        crate::agent_resume::PersistedAgentSession {
            source: self.source.clone(),
            agent: self.agent.clone(),
            session_ref: crate::agent_resume::AgentSessionRef {
                kind: self.kind,
                value: self.value.clone(),
            },
        }
    }
}

impl PaneAgentOwnerRefSnapshot {
    fn from_owner_ref(owner: &crate::agent_ownership::AgentOwnerRef) -> Self {
        Self {
            agent_id: owner.agent_id.clone(),
            name: owner.name.clone(),
            agent: owner.agent.clone(),
            session: owner
                .session
                .as_ref()
                .map(PaneAgentSessionSnapshot::from_session),
        }
    }

    pub fn to_owner_ref(&self) -> crate::agent_ownership::AgentOwnerRef {
        crate::agent_ownership::AgentOwnerRef {
            agent_id: self.agent_id.clone(),
            name: self.name.clone(),
            agent: self.agent.clone(),
            session: self
                .session
                .as_ref()
                .map(PaneAgentSessionSnapshot::to_session),
        }
    }
}

impl PaneAgentOwnershipSnapshot {
    fn from_ownership(ownership: &crate::agent_ownership::AgentOwnership) -> Self {
        Self {
            origin: PaneAgentOwnerRefSnapshot::from_owner_ref(&ownership.origin),
            current: ownership
                .current
                .as_ref()
                .map(PaneAgentOwnerRefSnapshot::from_owner_ref),
        }
    }

    pub fn to_ownership(&self) -> crate::agent_ownership::AgentOwnership {
        crate::agent_ownership::AgentOwnership {
            origin: self.origin.to_owner_ref(),
            current: self
                .current
                .as_ref()
                .map(PaneAgentOwnerRefSnapshot::to_owner_ref),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct PaneHistorySnapshot {
    pub ansi: String,
    pub lines: usize,
}

/// Serializable BSP tree.
#[derive(Serialize, Deserialize)]
pub enum LayoutSnapshot {
    Pane(u32),
    Split {
        direction: DirectionSnapshot,
        ratio: f32,
        first: Box<LayoutSnapshot>,
        second: Box<LayoutSnapshot>,
    },
}

#[derive(Serialize, Deserialize)]
pub enum DirectionSnapshot {
    Horizontal,
    Vertical,
}

impl From<LegacyWorkspaceSnapshot> for WorkspaceSnapshot {
    fn from(snap: LegacyWorkspaceSnapshot) -> Self {
        let identity_cwd = legacy_identity_cwd(&snap);
        let tab = TabSnapshot {
            custom_name: None,
            layout: snap.layout,
            panes: snap.panes,
            zoomed: snap.zoomed,
            focused: snap.focused,
            root_pane: snap.root_pane,
        };

        Self {
            id: None,
            custom_name: snap.custom_name,
            profiles: Vec::new(),
            identity_cwd,
            worktree_space: None,
            public_pane_numbers: HashMap::new(),
            next_public_pane_number: 0,
            public_tab_numbers: Vec::new(),
            next_public_tab_number: 0,
            tabs: vec![tab],
            active_tab: 0,
            orchestrator_mode: false,
        }
    }
}

#[derive(Deserialize)]
struct RawSessionSnapshot {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    workspaces: Vec<serde_json::Value>,
    #[serde(default)]
    active: Option<usize>,
    #[serde(default = "default_profile")]
    active_profile: String,
    #[serde(default)]
    selected: usize,
    #[serde(default)]
    sidebar_width: Option<u16>,
    #[serde(default)]
    sidebar_section_split: Option<f32>,
    #[serde(default)]
    collapsed_space_keys: std::collections::HashSet<String>,
    #[serde(default)]
    automations_expanded: bool,
    #[serde(default)]
    collapsed_agent_group_keys: std::collections::HashSet<String>,
    #[serde(default = "default_true")]
    tree_show_spaces: bool,
    #[serde(default = "default_true")]
    tree_show_tabs: bool,
    #[serde(default = "default_true")]
    tree_show_agents: bool,
    #[serde(default)]
    tree_collapsed_spaces: std::collections::HashSet<String>,
    #[serde(default)]
    tree_collapsed_tabs: std::collections::HashSet<String>,
    #[serde(default)]
    tree_pinned_spaces: std::collections::HashSet<String>,
}

fn migrate_snapshot(raw: RawSessionSnapshot) -> Result<SessionSnapshot, String> {
    Ok(SessionSnapshot {
        version: raw.version,
        workspaces: raw
            .workspaces
            .into_iter()
            .map(migrate_workspace)
            .collect::<Result<Vec<_>, _>>()?,
        active: raw.active,
        active_profile: crate::workspace::normalize_profile_name_lossy(&raw.active_profile)
            .unwrap_or_else(default_profile),
        selected: raw.selected,
        sidebar_width: raw.sidebar_width,
        sidebar_section_split: raw.sidebar_section_split,
        collapsed_space_keys: raw.collapsed_space_keys,
        automations_expanded: raw.automations_expanded,
        collapsed_agent_group_keys: raw.collapsed_agent_group_keys,
        tree_show_spaces: raw.tree_show_spaces,
        tree_show_tabs: raw.tree_show_tabs,
        tree_show_agents: raw.tree_show_agents,
        tree_collapsed_spaces: raw.tree_collapsed_spaces,
        tree_collapsed_tabs: raw.tree_collapsed_tabs,
        tree_pinned_spaces: raw.tree_pinned_spaces,
    })
}

fn migrate_workspace(raw: serde_json::Value) -> Result<WorkspaceSnapshot, String> {
    let mut snapshot: WorkspaceSnapshot = if raw.get("identity_cwd").is_some() {
        serde_json::from_value(raw).map_err(|e| e.to_string())?
    } else if raw.get("layout").is_some() {
        let legacy =
            serde_json::from_value::<LegacyWorkspaceSnapshot>(raw).map_err(|e| e.to_string())?;
        legacy.into()
    } else {
        return Err("workspace snapshot is neither current nor legacy format".to_string());
    };
    snapshot.profiles = crate::workspace::normalize_profiles_lossy(snapshot.profiles);
    Ok(snapshot)
}

fn legacy_identity_cwd(snap: &LegacyWorkspaceSnapshot) -> PathBuf {
    let root_pane = snap
        .root_pane
        .or_else(|| first_pane_id_in_layout(&snap.layout));

    root_pane
        .and_then(|pane_id| snap.panes.get(&pane_id))
        .map(|pane| pane.cwd.clone())
        .or_else(|| {
            first_pane_id_in_layout(&snap.layout)
                .and_then(|pane_id| snap.panes.get(&pane_id))
                .map(|pane| pane.cwd.clone())
        })
        .or_else(|| {
            snap.panes
                .keys()
                .min()
                .and_then(|pane_id| snap.panes.get(pane_id))
                .map(|pane| pane.cwd.clone())
        })
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| "/".into()))
}

fn owner_session_by_identity(
    terminals: &HashMap<crate::terminal::TerminalId, crate::terminal::TerminalState>,
    agent_id: &str,
) -> Option<PaneAgentSessionSnapshot> {
    terminals
        .values()
        .find(|terminal| terminal.agent_identity.as_deref() == Some(agent_id))
        .and_then(|terminal| terminal.current_agent_session())
        .map(|session| PaneAgentSessionSnapshot::from_session(&session))
}

fn first_pane_id_in_layout(layout: &LayoutSnapshot) -> Option<u32> {
    match layout {
        LayoutSnapshot::Pane(id) => Some(*id),
        LayoutSnapshot::Split { first, second, .. } => {
            first_pane_id_in_layout(first).or_else(|| first_pane_id_in_layout(second))
        }
    }
}

/// UI preferences captured alongside the session. Bundled into one struct so
/// `capture` stops growing a positional argument per preference (this retires
/// the 16-positional-args debt from the tree-view work).
pub struct UiPrefs {
    pub sidebar_width: u16,
    pub sidebar_section_split: f32,
    pub collapsed_space_keys: std::collections::HashSet<String>,
    pub automations_expanded: bool,
    pub collapsed_agent_group_keys: std::collections::HashSet<String>,
    pub tree_show_spaces: bool,
    pub tree_show_tabs: bool,
    pub tree_show_agents: bool,
    pub tree_collapsed_spaces: std::collections::HashSet<String>,
    pub tree_collapsed_tabs: std::collections::HashSet<String>,
    pub tree_pinned_spaces: std::collections::HashSet<String>,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            sidebar_width: 0,
            sidebar_section_split: 0.5,
            collapsed_space_keys: Default::default(),
            automations_expanded: false,
            collapsed_agent_group_keys: Default::default(),
            tree_show_spaces: true,
            tree_show_tabs: true,
            tree_show_agents: true,
            tree_collapsed_spaces: Default::default(),
            tree_collapsed_tabs: Default::default(),
            tree_pinned_spaces: Default::default(),
        }
    }
}

/// Capture the current app state into a serializable snapshot.
pub fn capture(
    workspaces: &[Workspace],
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
    terminal_runtimes: &TerminalRuntimeRegistry,
    active: Option<usize>,
    active_profile: String,
    selected: usize,
    ui: UiPrefs,
) -> SessionSnapshot {
    SessionSnapshot {
        version: SNAPSHOT_VERSION,
        workspaces: workspaces
            .iter()
            .map(|workspace| capture_workspace(workspace, terminals, terminal_runtimes))
            .collect(),
        active,
        active_profile,
        selected,
        sidebar_width: Some(ui.sidebar_width),
        sidebar_section_split: Some(ui.sidebar_section_split),
        collapsed_space_keys: ui.collapsed_space_keys,
        automations_expanded: ui.automations_expanded,
        collapsed_agent_group_keys: ui.collapsed_agent_group_keys,
        tree_show_spaces: ui.tree_show_spaces,
        tree_show_tabs: ui.tree_show_tabs,
        tree_show_agents: ui.tree_show_agents,
        tree_collapsed_spaces: ui.tree_collapsed_spaces,
        tree_collapsed_tabs: ui.tree_collapsed_tabs,
        tree_pinned_spaces: ui.tree_pinned_spaces,
    }
}

fn capture_workspace(
    ws: &Workspace,
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
    terminal_runtimes: &TerminalRuntimeRegistry,
) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        id: Some(ws.id.clone()),
        custom_name: ws.custom_name.clone(),
        profiles: ws.profiles.clone(),
        identity_cwd: ws
            .resolved_identity_cwd_from(terminals, terminal_runtimes)
            .unwrap_or_else(|| ws.identity_cwd.clone()),
        worktree_space: ws.worktree_space.clone(),
        public_pane_numbers: ws
            .public_pane_numbers
            .iter()
            .map(|(pane_id, number)| (pane_id.raw(), *number))
            .collect(),
        next_public_pane_number: ws.next_public_pane_number,
        public_tab_numbers: ws.tabs.iter().map(|tab| tab.number).collect(),
        next_public_tab_number: ws.next_public_tab_number,
        tabs: ws
            .tabs
            .iter()
            .map(|tab| capture_tab(tab, terminals, terminal_runtimes))
            .collect(),
        active_tab: ws.active_tab,
        orchestrator_mode: ws.orchestrator_mode,
    }
}

fn capture_tab(
    tab: &crate::workspace::Tab,
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
    terminal_runtimes: &TerminalRuntimeRegistry,
) -> TabSnapshot {
    let mut panes = HashMap::new();
    for id in tab.panes.keys() {
        let cwd = tab
            .cwd_for_pane(*id, terminals, terminal_runtimes)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| "/".into()));
        let terminal = tab
            .panes
            .get(id)
            .and_then(|pane| terminals.get(&pane.attached_terminal_id));
        let label = terminal.and_then(|terminal| terminal.manual_label.clone());
        let (agent_name, managed_agent_kind) = terminal
            .filter(|terminal| !terminal.managed_agent_launch_pending())
            .map(|terminal| {
                (
                    terminal.agent_name.clone(),
                    terminal
                        .managed_agent_kind()
                        .map(|agent| crate::detect::agent_label(agent).to_string()),
                )
            })
            .unwrap_or_default();
        let launch_argv = terminal.and_then(|terminal| terminal.launch_argv.clone());
        let agent_session = terminal.and_then(|terminal| {
            if let Some(authority) = terminal.hook_authority.as_ref() {
                if let Some(session_ref) = authority.session_ref.as_ref() {
                    return Some(PaneAgentSessionSnapshot {
                        source: authority.source.clone(),
                        agent: authority.agent_label.clone(),
                        kind: session_ref.kind,
                        value: session_ref.value.clone(),
                    });
                }
            }
            terminal
                .persisted_agent_session
                .as_ref()
                .map(|session| PaneAgentSessionSnapshot {
                    source: session.source.clone(),
                    agent: session.agent.clone(),
                    kind: session.session_ref.kind,
                    value: session.session_ref.value.clone(),
                })
        });
        let profiles = terminal
            .map(|terminal| terminal.profiles.clone())
            .unwrap_or_default();
        let agent_identity = terminal.and_then(|terminal| terminal.agent_identity.clone());
        let agent_ownership = terminal.and_then(|terminal| {
            terminal.agent_ownership.as_ref().map(|ownership| {
                let mut snapshot = PaneAgentOwnershipSnapshot::from_ownership(ownership);
                // The owner's session identity may appear after ownership was
                // captured; embed the latest known session so the reference can
                // be reconciled if the owner's session is resumed elsewhere.
                if let Some(current) = snapshot.current.as_mut() {
                    if current.session.is_none() {
                        current.session = owner_session_by_identity(terminals, &current.agent_id);
                    }
                }
                snapshot
            })
        });
        let terminal_title = terminal.and_then(|terminal| terminal.terminal_title.clone());
        panes.insert(
            id.raw(),
            PaneSnapshot {
                cwd,
                label,
                agent_name,
                managed_agent_kind,
                agent_session,
                launch_argv,
                agent_identity,
                agent_ownership,
                profiles,
                terminal_title,
            },
        );
    }
    TabSnapshot {
        custom_name: tab.custom_name.clone(),
        layout: capture_node(tab.layout.root()),
        panes,
        zoomed: tab.zoomed,
        focused: Some(tab.layout.focused().raw()),
        root_pane: Some(tab.root_pane.raw()),
    }
}

/// Capture pane screen history separately from the structural session snapshot.
pub fn capture_history(
    workspaces: &[Workspace],
    terminal_runtimes: &TerminalRuntimeRegistry,
) -> SessionHistorySnapshot {
    SessionHistorySnapshot {
        version: SNAPSHOT_VERSION,
        workspaces: workspaces
            .iter()
            .map(|workspace| WorkspaceHistorySnapshot {
                tabs: workspace
                    .tabs
                    .iter()
                    .map(|tab| TabHistorySnapshot {
                        panes: capture_tab_history(tab, terminal_runtimes),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn capture_tab_history(
    tab: &crate::workspace::Tab,
    terminal_runtimes: &TerminalRuntimeRegistry,
) -> HashMap<u32, PaneHistorySnapshot> {
    let mut panes = HashMap::new();
    for (id, pane) in &tab.panes {
        if let Some(history) = capture_pane_history(Some(pane), terminal_runtimes) {
            panes.insert(id.raw(), history);
        }
    }
    panes
}

fn capture_pane_history(
    pane: Option<&crate::pane::PaneState>,
    terminal_runtimes: &TerminalRuntimeRegistry,
) -> Option<PaneHistorySnapshot> {
    let ansi = terminal_runtimes
        .get(&pane?.attached_terminal_id)?
        .snapshot_history()?;
    let lines = ansi.lines().count();
    Some(PaneHistorySnapshot { ansi, lines })
}

pub(super) fn capture_node(node: &Node) -> LayoutSnapshot {
    match node {
        Node::Pane(id) => LayoutSnapshot::Pane(id.raw()),
        Node::Split {
            direction,
            ratio,
            first,
            second,
        } => LayoutSnapshot::Split {
            direction: match direction {
                Direction::Horizontal => DirectionSnapshot::Horizontal,
                Direction::Vertical => DirectionSnapshot::Vertical,
            },
            ratio: *ratio,
            first: Box::new(capture_node(first)),
            second: Box::new(capture_node(second)),
        },
    }
}

pub(super) fn parse_snapshot(content: &str) -> Result<SessionSnapshot, String> {
    let raw = serde_json::from_str::<RawSessionSnapshot>(content).map_err(|e| e.to_string())?;
    if raw.version > SNAPSHOT_VERSION {
        return Err(format!(
            "snapshot version {} is newer than supported {}",
            raw.version, SNAPSHOT_VERSION
        ));
    }
    migrate_snapshot(raw)
}

pub(super) fn parse_history_snapshot(content: &str) -> Result<SessionHistorySnapshot, String> {
    let snapshot =
        serde_json::from_str::<SessionHistorySnapshot>(content).map_err(|e| e.to_string())?;
    if snapshot.version > SNAPSHOT_VERSION {
        return Err(format!(
            "history snapshot version {} is newer than supported {}",
            snapshot.version, SNAPSHOT_VERSION
        ));
    }
    Ok(snapshot)
}

pub(super) fn snapshot_file_version(content: &str) -> Option<u32> {
    serde_json::from_str::<RawSessionSnapshot>(content)
        .ok()
        .map(|raw| raw.version)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use ratatui::layout::{Direction, Rect};

    use super::*;
    use crate::app::{AppState, Mode};
    use crate::layout::NavDirection;
    use crate::workspace::Workspace;

    fn session_fixture(name: &str) -> &'static str {
        match name {
            "current-herdr" => {
                include_str!("../../tests/fixtures/session/current-herdr-session.json")
            }
            "current-herdr-dev" => {
                include_str!("../../tests/fixtures/session/current-herdr-dev-session.json")
            }
            "legacy-pre-tabs-v2" => {
                include_str!("../../tests/fixtures/session/legacy-pre-tabs-v2.json")
            }
            other => panic!("unknown session fixture: {other}"),
        }
    }

    fn test_session_path(name: &str) -> String {
        std::env::current_dir()
            .unwrap()
            .join(name)
            .display()
            .to_string()
    }

    fn state_with_workspaces(names: &[&str]) -> AppState {
        let mut state = AppState::test_new();
        state.workspaces = names.iter().map(|name| Workspace::test_new(name)).collect();
        state.ensure_test_terminals();
        if !state.workspaces.is_empty() {
            state.active = Some(0);
            state.selected = 0;
            state.replace_mode(Mode::Terminal);
        }
        state
    }

    #[test]
    fn legacy_default_active_profile_migrates_to_personal() {
        let snapshot = capture_from_state(&state_with_workspaces(&["one"]));
        let mut serialized = serde_json::to_value(snapshot).unwrap();
        serialized["active_profile"] = serde_json::Value::String("default".into());

        let migrated = parse_snapshot(&serialized.to_string()).unwrap();

        assert_eq!(migrated.active_profile, "personal");
    }

    #[test]
    fn legacy_default_workspace_profiles_migrate_and_deduplicate() {
        for (profiles, expected) in [
            (vec!["default", "work"], vec!["personal", "work"]),
            (vec!["default", "personal"], vec!["personal"]),
        ] {
            let snapshot = capture_from_state(&state_with_workspaces(&["one"]));
            let mut serialized = serde_json::to_value(snapshot).unwrap();
            serialized["workspaces"][0]["profiles"] = serde_json::json!(profiles);

            let migrated = parse_snapshot(&serialized.to_string()).unwrap();

            assert_eq!(migrated.workspaces[0].profiles, expected);
        }
    }

    #[test]
    fn missing_active_profile_defaults_to_personal() {
        let snapshot = capture_from_state(&state_with_workspaces(&["one"]));
        let mut serialized = serde_json::to_value(snapshot).unwrap();
        serialized.as_object_mut().unwrap().remove("active_profile");

        let migrated = parse_snapshot(&serialized.to_string()).unwrap();

        assert_eq!(migrated.active_profile, "personal");
    }

    #[test]
    fn invalid_active_profile_defaults_to_personal() {
        let snapshot = capture_from_state(&state_with_workspaces(&["one"]));
        let mut serialized = serde_json::to_value(snapshot).unwrap();
        serialized["active_profile"] = serde_json::Value::String(String::new());

        let migrated = parse_snapshot(&serialized.to_string()).unwrap();

        assert_eq!(migrated.active_profile, "personal");
    }

    #[test]
    fn invalid_workspace_profiles_are_dropped() {
        let snapshot = capture_from_state(&state_with_workspaces(&["one"]));
        let mut serialized = serde_json::to_value(snapshot).unwrap();
        serialized["workspaces"][0]["profiles"] = serde_json::json!(["", "wörk", "work"]);

        let migrated = parse_snapshot(&serialized.to_string()).unwrap();

        assert_eq!(migrated.workspaces[0].profiles, ["work"]);
    }

    #[test]
    fn excessive_workspace_profiles_are_truncated() {
        let snapshot = capture_from_state(&state_with_workspaces(&["one"]));
        let mut serialized = serde_json::to_value(snapshot).unwrap();
        let profiles = (0..=crate::workspace::MAX_WORKSPACE_PROFILES)
            .map(|idx| format!("p{idx}"))
            .collect::<Vec<_>>();
        serialized["workspaces"][0]["profiles"] = serde_json::json!(profiles);

        let migrated = parse_snapshot(&serialized.to_string()).unwrap();

        assert_eq!(
            migrated.workspaces[0].profiles,
            profiles[..crate::workspace::MAX_WORKSPACE_PROFILES]
        );
    }

    #[test]
    fn automations_expanded_roundtrips_and_defaults_false_when_missing() {
        let mut state = state_with_workspaces(&["one"]);
        state.automations_expanded = true;
        let snapshot = capture_from_state(&state);
        let serialized = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(serialized["automations_expanded"], true);

        let mut old = serialized;
        old.as_object_mut().unwrap().remove("automations_expanded");
        let migrated = migrate_snapshot(serde_json::from_value(old).unwrap()).unwrap();
        assert!(!migrated.automations_expanded);
    }

    fn capture_from_state(state: &AppState) -> SessionSnapshot {
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        capture_from_state_with_runtimes(state, &terminal_runtimes)
    }

    fn capture_from_state_with_runtimes(
        state: &AppState,
        terminal_runtimes: &TerminalRuntimeRegistry,
    ) -> SessionSnapshot {
        capture(
            &state.workspaces,
            &state.terminals,
            terminal_runtimes,
            state.active,
            state.active_profile.clone(),
            state.selected,
            state.snapshot_ui_prefs(),
        )
    }

    fn capture_history_from_state_with_runtimes(
        state: &AppState,
        terminal_runtimes: &TerminalRuntimeRegistry,
    ) -> SessionHistorySnapshot {
        capture_history(&state.workspaces, terminal_runtimes)
    }

    fn root_split_ratio(tab: &TabSnapshot) -> Option<f32> {
        match &tab.layout {
            LayoutSnapshot::Split { ratio, .. } => Some(*ratio),
            LayoutSnapshot::Pane(_) => None,
        }
    }

    #[test]
    fn managed_agent_snapshot_omits_pending_and_persists_active_ownership() {
        let mut state = state_with_workspaces(&["managed-snapshot"]);
        let root = state.workspaces[0].tabs[0].root_pane;
        let terminal_id = state.workspaces[0].tabs[0].panes[&root]
            .attached_terminal_id
            .clone();
        let now = std::time::Instant::now();
        state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .begin_managed_agent(
                "reviewer".into(),
                crate::detect::Agent::Pi,
                now,
                std::time::Duration::ZERO,
                std::time::Duration::from_secs(1),
            );

        let pending = capture_from_state(&state);
        let pending_pane = &pending.workspaces[0].tabs[0].panes[&root.raw()];
        assert_eq!(pending_pane.agent_name, None);
        assert_eq!(pending_pane.managed_agent_kind, None);

        let terminal = state.terminals.get_mut(&terminal_id).unwrap();
        terminal.set_detected_state(
            Some(crate::detect::Agent::Pi),
            crate::detect::AgentState::Idle,
        );
        assert!(terminal.reconcile_managed_agent_at(now, false));
        let active = capture_from_state(&state);
        let active_pane = &active.workspaces[0].tabs[0].panes[&root.raw()];
        assert_eq!(active_pane.agent_name.as_deref(), Some("reviewer"));
        assert_eq!(active_pane.managed_agent_kind.as_deref(), Some("pi"));
    }

    #[test]
    fn round_trip_empty_session() {
        let snap = SessionSnapshot {
            version: SNAPSHOT_VERSION,
            workspaces: vec![],
            active: None,
            active_profile: crate::workspace::DEFAULT_PROFILE.to_string(),
            selected: 0,
            sidebar_width: Some(26),
            sidebar_section_split: Some(0.5),
            collapsed_space_keys: std::collections::HashSet::new(),
            automations_expanded: false,
            collapsed_agent_group_keys: std::collections::HashSet::new(),
            tree_show_spaces: true,
            tree_show_tabs: true,
            tree_show_agents: true,
            tree_collapsed_spaces: std::collections::HashSet::new(),
            tree_collapsed_tabs: std::collections::HashSet::new(),
            tree_pinned_spaces: std::collections::HashSet::new(),
        };
        let json = serde_json::to_string(&snap).unwrap();
        let restored = parse_snapshot(&json).unwrap();
        assert!(restored.workspaces.is_empty());
        assert_eq!(restored.active, None);
        assert_eq!(restored.sidebar_width, Some(26));
        assert_eq!(restored.sidebar_section_split, Some(0.5));
    }

    #[test]
    fn pane_profiles_roundtrip_and_default_empty_when_missing() {
        let mut state = state_with_workspaces(&["one"]);
        let pane_id = state.workspaces[0].tabs[0].root_pane;
        let terminal_id = state.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        state.terminals.get_mut(&terminal_id).unwrap().profiles = vec!["work".into()];
        let snapshot = capture_from_state(&state);
        let mut serialized = serde_json::to_value(snapshot).unwrap();

        let restored = parse_snapshot(&serialized.to_string()).unwrap();
        assert_eq!(
            restored.workspaces[0].tabs[0].panes[&pane_id.raw()].profiles,
            ["work"]
        );

        serialized["workspaces"][0]["tabs"][0]["panes"][pane_id.raw().to_string()]
            .as_object_mut()
            .unwrap()
            .remove("profiles");
        let restored = parse_snapshot(&serialized.to_string()).unwrap();
        assert!(restored.workspaces[0].tabs[0].panes[&pane_id.raw()]
            .profiles
            .is_empty());
    }

    #[test]
    fn pane_profiles_are_normalized_during_snapshot_load() {
        let state = state_with_workspaces(&["one"]);
        let pane_id = state.workspaces[0].tabs[0].root_pane;
        let snapshot = capture_from_state(&state);
        let mut serialized = serde_json::to_value(snapshot).unwrap();
        serialized["workspaces"][0]["tabs"][0]["panes"][pane_id.raw().to_string()]["profiles"] =
            serde_json::json!([" work ", "work", "wörk"]);

        let restored = parse_snapshot(&serialized.to_string()).unwrap();

        assert_eq!(
            restored.workspaces[0].tabs[0].panes[&pane_id.raw()].profiles,
            ["work"]
        );
    }

    #[test]
    fn round_trip_layout_snapshot() {
        let layout = LayoutSnapshot::Split {
            direction: DirectionSnapshot::Horizontal,
            ratio: 0.6,
            first: Box::new(LayoutSnapshot::Pane(0)),
            second: Box::new(LayoutSnapshot::Split {
                direction: DirectionSnapshot::Vertical,
                ratio: 0.5,
                first: Box::new(LayoutSnapshot::Pane(1)),
                second: Box::new(LayoutSnapshot::Pane(2)),
            }),
        };
        let json = serde_json::to_string(&layout).unwrap();
        let restored: LayoutSnapshot = serde_json::from_str(&json).unwrap();

        match restored {
            LayoutSnapshot::Split { ratio, .. } => assert!((ratio - 0.6).abs() < 0.01),
            _ => panic!("expected split"),
        }
    }

    #[test]
    fn round_trip_full_workspace_snapshot() {
        let mut panes = HashMap::new();
        panes.insert(
            0,
            PaneSnapshot {
                terminal_title: None,
                cwd: PathBuf::from("/home/can/Projects/herdr"),
                label: None,
                agent_name: None,
                managed_agent_kind: None,
                agent_session: None,
                launch_argv: None,
                agent_identity: None,
                agent_ownership: None,
                profiles: Vec::new(),
            },
        );
        panes.insert(
            1,
            PaneSnapshot {
                terminal_title: None,
                cwd: PathBuf::from("/home/can/Projects/website"),
                label: Some("website".into()),
                agent_name: None,
                managed_agent_kind: None,
                agent_session: None,
                launch_argv: None,
                agent_identity: None,
                agent_ownership: None,
                profiles: Vec::new(),
            },
        );

        let snap = SessionSnapshot {
            workspaces: vec![WorkspaceSnapshot {
                id: Some("wproj".to_string()),
                custom_name: Some("pi-mono".to_string()),
                profiles: Vec::new(),
                identity_cwd: PathBuf::from("/home/can/Projects/herdr"),
                worktree_space: None,
                public_pane_numbers: HashMap::from([(0, 1), (1, 2)]),
                next_public_pane_number: 3,
                public_tab_numbers: vec![1],
                next_public_tab_number: 2,
                tabs: vec![TabSnapshot {
                    custom_name: Some("api".to_string()),
                    layout: LayoutSnapshot::Split {
                        direction: DirectionSnapshot::Horizontal,
                        ratio: 0.5,
                        first: Box::new(LayoutSnapshot::Pane(0)),
                        second: Box::new(LayoutSnapshot::Pane(1)),
                    },
                    panes,
                    zoomed: false,
                    focused: Some(0),
                    root_pane: Some(0),
                }],
                active_tab: 0,
                orchestrator_mode: false,
            }],
            active: Some(0),
            active_profile: crate::workspace::DEFAULT_PROFILE.to_string(),
            selected: 0,
            sidebar_width: Some(26),
            sidebar_section_split: Some(0.5),
            collapsed_space_keys: std::collections::HashSet::new(),
            automations_expanded: false,
            collapsed_agent_group_keys: std::collections::HashSet::new(),
            tree_show_spaces: true,
            tree_show_tabs: true,
            tree_show_agents: true,
            tree_collapsed_spaces: std::collections::HashSet::new(),
            tree_collapsed_tabs: std::collections::HashSet::new(),
            tree_pinned_spaces: std::collections::HashSet::new(),
            version: SNAPSHOT_VERSION,
        };

        let json = serde_json::to_string_pretty(&snap).unwrap();
        let restored = parse_snapshot(&json).unwrap();

        assert_eq!(restored.workspaces.len(), 1);
        assert_eq!(restored.workspaces[0].id.as_deref(), Some("wproj"));
        assert_eq!(
            restored.workspaces[0].custom_name.as_deref(),
            Some("pi-mono")
        );
        assert_eq!(restored.workspaces[0].tabs.len(), 1);
        assert_eq!(restored.workspaces[0].tabs[0].panes.len(), 2);
        assert_eq!(
            restored.workspaces[0].tabs[0].panes[&0].cwd,
            PathBuf::from("/home/can/Projects/herdr")
        );
        assert_eq!(
            restored.workspaces[0].tabs[0].panes[&1].label.as_deref(),
            Some("website")
        );
        assert_eq!(restored.sidebar_width, Some(26));
        assert_eq!(restored.sidebar_section_split, Some(0.5));
    }

    #[test]
    fn orchestrator_fields_round_trip_and_default_off() {
        let mut panes = HashMap::new();
        panes.insert(
            0,
            PaneSnapshot {
                terminal_title: None,
                cwd: PathBuf::from("/repo"),
                label: None,
                agent_name: None,
                managed_agent_kind: None,
                agent_session: None,
                launch_argv: None,
                agent_identity: None,
                agent_ownership: None,
                profiles: Vec::new(),
            },
        );
        let snap = SessionSnapshot {
            workspaces: vec![WorkspaceSnapshot {
                id: Some("worch".to_string()),
                custom_name: None,
                profiles: Vec::new(),
                identity_cwd: PathBuf::from("/repo"),
                worktree_space: None,
                public_pane_numbers: HashMap::from([(0, 1)]),
                next_public_pane_number: 2,
                public_tab_numbers: vec![1],
                next_public_tab_number: 2,
                tabs: vec![TabSnapshot {
                    custom_name: None,
                    layout: LayoutSnapshot::Pane(0),
                    panes,
                    zoomed: false,
                    focused: Some(0),
                    root_pane: Some(0),
                }],
                active_tab: 0,
                orchestrator_mode: true,
            }],
            active: Some(0),
            active_profile: crate::workspace::DEFAULT_PROFILE.to_string(),
            selected: 0,
            sidebar_width: None,
            sidebar_section_split: None,
            collapsed_space_keys: std::collections::HashSet::new(),
            automations_expanded: false,
            collapsed_agent_group_keys: std::collections::HashSet::new(),
            tree_show_spaces: true,
            tree_show_tabs: true,
            tree_show_agents: true,
            tree_collapsed_spaces: std::collections::HashSet::new(),
            tree_collapsed_tabs: std::collections::HashSet::new(),
            tree_pinned_spaces: std::collections::HashSet::new(),
            version: SNAPSHOT_VERSION,
        };

        let json = serde_json::to_string(&snap).unwrap();
        let restored = parse_snapshot(&json).unwrap();
        assert!(restored.workspaces[0].orchestrator_mode);

        // Snapshots written before the feature omit the field entirely.
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["workspaces"][0]
            .as_object_mut()
            .unwrap()
            .remove("orchestrator_mode");
        let restored = parse_snapshot(&value.to_string()).unwrap();
        assert!(!restored.workspaces[0].orchestrator_mode);
    }

    #[test]
    fn current_session_fixture_parses() {
        let snap = parse_snapshot(session_fixture("current-herdr")).unwrap();

        assert_eq!(snap.version, 3);
        assert_eq!(snap.workspaces.len(), 2);
        assert_eq!(snap.active, Some(0));
        assert_eq!(snap.active_profile, DEFAULT_PROFILE);
        assert!(snap
            .workspaces
            .iter()
            .all(|workspace| workspace.profiles.is_empty()));
        assert_eq!(snap.selected, 0);
        assert_eq!(snap.sidebar_width, None);
        assert_eq!(snap.sidebar_section_split, None);
        assert_eq!(snap.workspaces[0].tabs.len(), 2);
        assert_eq!(
            snap.workspaces[1].identity_cwd,
            PathBuf::from("/home/test/projects/project-b")
        );
    }

    #[test]
    fn current_dev_session_fixture_parses_additive_fields() {
        let snap = parse_snapshot(session_fixture("current-herdr-dev")).unwrap();

        assert_eq!(snap.version, 3);
        assert_eq!(snap.workspaces.len(), 2);
        assert_eq!(snap.sidebar_section_split, Some(0.4));
        assert_eq!(snap.workspaces[0].active_tab, 1);
        assert_eq!(snap.workspaces[1].tabs[0].panes.len(), 2);
    }

    #[test]
    fn old_snapshot_defaults_sidebar_fields() {
        let json = serde_json::json!({
            "version": SNAPSHOT_VERSION,
            "workspaces": [],
            "active": null,
            "selected": 0
        })
        .to_string();

        let restored = parse_snapshot(&json).unwrap();

        assert_eq!(restored.sidebar_width, None);
        assert_eq!(restored.sidebar_section_split, None);
    }

    #[test]
    fn old_pane_snapshot_with_embedded_history_is_ignored() {
        let json = serde_json::json!({
            "version": SNAPSHOT_VERSION,
            "workspaces": [{
                "id": "wtest",
                "identity_cwd": "/tmp",
                "tabs": [{
                    "layout": { "Pane": 0 },
                    "panes": {
                        "0": {
                            "cwd": "/tmp",
                            "history": {
                                "ansi": "legacy-secret",
                                "lines": 1
                            }
                        }
                    },
                    "zoomed": false,
                    "focused": 0,
                    "root_pane": 0
                }],
                "active_tab": 0
            }],
            "active": 0,
            "selected": 0
        })
        .to_string();

        let restored = parse_snapshot(&json).unwrap();

        let encoded = serde_json::to_string(&restored).unwrap();
        assert!(!encoded.contains("legacy-secret"));
        assert!(!encoded.contains("\"history\""));
    }

    #[test]
    fn legacy_workspace_snapshot_migrates_to_single_tab() {
        let snap = parse_snapshot(session_fixture("legacy-pre-tabs-v2")).unwrap();
        let ws = &snap.workspaces[0];

        assert_eq!(snap.version, 2);
        assert_eq!(snap.active_profile, DEFAULT_PROFILE);
        assert_eq!(snap.workspaces.len(), 1);
        assert!(ws.profiles.is_empty());
        assert_eq!(ws.custom_name.as_deref(), Some("legacy"));
        assert_eq!(ws.identity_cwd, PathBuf::from("/tmp/pion"));
        assert_eq!(ws.active_tab, 0);
        assert_eq!(ws.tabs.len(), 1);
        assert_eq!(ws.tabs[0].focused, Some(1));
        assert_eq!(ws.tabs[0].root_pane, Some(0));
        assert_eq!(ws.tabs[0].panes[&0].cwd, PathBuf::from("/tmp/pion"));
        assert_eq!(ws.tabs[0].panes[&1].cwd, PathBuf::from("/tmp/herdr"));
    }

    #[test]
    fn profile_fields_survive_capture_and_json_roundtrip() {
        let mut state = state_with_workspaces(&["shared"]);
        state.active_profile = "work".into();
        state.workspaces[0].profiles = vec!["work".into(), "personal".into()];

        let captured = capture_from_state(&state);
        let json = serde_json::to_string(&captured).unwrap();
        let restored = parse_snapshot(&json).unwrap();

        assert_eq!(restored.active_profile, "work");
        assert_eq!(restored.workspaces[0].profiles, vec!["work", "personal"]);
    }

    #[test]
    fn capture_contract_tracks_workspace_order_active_and_selected() {
        let mut state = state_with_workspaces(&["a", "b", "c"]);
        state.active = Some(1);
        state.selected = 2;

        state.move_workspace(1, 0);

        let snapshot = capture_from_state(&state);
        let ids: Vec<_> = state.workspaces.iter().map(|ws| ws.id.clone()).collect();
        let captured_ids: Vec<_> = snapshot
            .workspaces
            .iter()
            .map(|ws| ws.id.clone().unwrap())
            .collect();
        assert_eq!(captured_ids, ids);
        assert_eq!(snapshot.active, state.active);
        assert_eq!(snapshot.selected, state.selected);
    }

    #[test]
    fn capture_contract_tracks_workspace_and_tab_names_and_active_tab() {
        let mut state = state_with_workspaces(&["one"]);
        state.workspaces[0].set_custom_name("renamed-workspace".into());
        let second_tab = state.workspaces[0].test_add_tab(Some("logs"));
        state.workspaces[0].switch_tab(second_tab);
        state.workspaces[0].tabs[0].set_custom_name("main".into());

        let snapshot = capture_from_state(&state);
        let workspace = &snapshot.workspaces[0];
        assert_eq!(workspace.custom_name.as_deref(), Some("renamed-workspace"));
        assert_eq!(workspace.active_tab, second_tab);
        assert_eq!(workspace.tabs[0].custom_name.as_deref(), Some("main"));
        assert_eq!(workspace.tabs[1].custom_name.as_deref(), Some("logs"));
    }

    #[test]
    fn capture_contract_tracks_workspace_closure() {
        let mut state = state_with_workspaces(&["one", "two"]);
        state.selected = 1;
        state.active = Some(1);

        state.close_selected_workspace();

        let snapshot = capture_from_state(&state);
        assert_eq!(snapshot.workspaces.len(), 1);
        assert_eq!(snapshot.workspaces[0].custom_name.as_deref(), Some("one"));
        assert_eq!(snapshot.active, Some(0));
        assert_eq!(snapshot.selected, 0);
    }

    #[test]
    fn capture_contract_tracks_sidebar_state() {
        let mut state = state_with_workspaces(&["one"]);
        state.sidebar_width = 31;
        state.sidebar_section_split = 0.4;
        state.collapsed_space_keys.insert("repo-key".into());

        let snapshot = capture_from_state(&state);
        assert_eq!(snapshot.sidebar_width, Some(31));
        assert_eq!(snapshot.sidebar_section_split, Some(0.4));
        assert!(snapshot.collapsed_space_keys.contains("repo-key"));
    }

    #[test]
    fn capture_contract_tracks_collapsed_agent_group_keys() {
        let mut state = state_with_workspaces(&["one"]);
        state.collapsed_agent_group_keys.insert("agent_lead".into());

        let snapshot = capture_from_state(&state);
        assert!(snapshot.collapsed_agent_group_keys.contains("agent_lead"));

        let json = serde_json::to_string(&snapshot).unwrap();
        let restored = parse_snapshot(&json).unwrap();
        assert!(restored.collapsed_agent_group_keys.contains("agent_lead"));
    }

    #[test]
    fn capture_contract_tracks_tree_state() {
        let mut state = state_with_workspaces(&["one"]);
        state.tree_show_spaces = false;
        state.tree_show_tabs = false;
        state.tree_show_agents = false;
        state.tree_collapsed_spaces.insert("repo-key".into());
        state.tree_collapsed_tabs.insert("repo-key#0".into());
        state.tree_pinned_spaces.insert("repo-key".into());

        let snapshot = capture_from_state(&state);
        assert!(!snapshot.tree_show_spaces);
        assert!(!snapshot.tree_show_tabs);
        assert!(!snapshot.tree_show_agents);
        assert!(snapshot.tree_collapsed_spaces.contains("repo-key"));
        assert!(snapshot.tree_collapsed_tabs.contains("repo-key#0"));
        assert!(snapshot.tree_pinned_spaces.contains("repo-key"));

        let json = serde_json::to_string(&snapshot).unwrap();
        let restored = parse_snapshot(&json).unwrap();
        assert!(!restored.tree_show_spaces);
        assert!(!restored.tree_show_tabs);
        assert!(!restored.tree_show_agents);
        assert!(restored.tree_collapsed_spaces.contains("repo-key"));
        assert!(restored.tree_collapsed_tabs.contains("repo-key#0"));
        assert!(restored.tree_pinned_spaces.contains("repo-key"));
    }

    #[test]
    fn tree_show_flags_default_true_when_missing() {
        let state = state_with_workspaces(&["one"]);
        let snapshot = capture_from_state(&state);
        let serialized = serde_json::to_value(&snapshot).unwrap();

        let mut old = serialized;
        let obj = old.as_object_mut().unwrap();
        obj.remove("tree_show_spaces");
        obj.remove("tree_show_tabs");
        obj.remove("tree_show_agents");
        let migrated = migrate_snapshot(serde_json::from_value(old).unwrap()).unwrap();
        assert!(migrated.tree_show_spaces);
        assert!(migrated.tree_show_tabs);
        assert!(migrated.tree_show_agents);
    }

    #[test]
    fn capture_records_agent_ownership_and_embeds_owner_session() {
        let mut state = state_with_workspaces(&["own", "wkr"]);

        let owner_pane = state.workspaces[0].tabs[0].root_pane;
        let owner_terminal_id = state.workspaces[0].tabs[0].panes[&owner_pane]
            .attached_terminal_id
            .clone();
        let owner = state.terminals.get_mut(&owner_terminal_id).unwrap();
        owner.set_agent_name("lead".into());
        owner.set_detected_state(
            Some(crate::detect::Agent::Pi),
            crate::detect::AgentState::Idle,
        );
        owner.agent_identity = Some("agent_lead".into());
        owner.set_persisted_agent_session(crate::agent_resume::PersistedAgentSession {
            source: "herdr:pi".into(),
            agent: "pi".into(),
            session_ref: crate::agent_resume::AgentSessionRef::id("lead-session").unwrap(),
        });

        let worker_pane = state.workspaces[1].tabs[0].root_pane;
        let worker_terminal_id = state.workspaces[1].tabs[0].panes[&worker_pane]
            .attached_terminal_id
            .clone();
        let worker = state.terminals.get_mut(&worker_terminal_id).unwrap();
        worker.set_agent_name("worker".into());
        worker.set_detected_state(
            Some(crate::detect::Agent::Pi),
            crate::detect::AgentState::Idle,
        );
        worker.agent_identity = Some("agent_worker".into());
        // Ownership captured before the owner's session ref was known.
        let owner_ref = crate::agent_ownership::AgentOwnerRef {
            agent_id: "agent_lead".into(),
            name: Some("lead".into()),
            agent: Some("pi".into()),
            session: None,
        };
        worker.agent_ownership = Some(crate::agent_ownership::AgentOwnership::new(owner_ref));

        let snapshot = capture_from_state(&state);
        let worker_snapshot = snapshot.workspaces[1].tabs[0]
            .panes
            .get(&worker_pane.raw())
            .unwrap();
        assert_eq!(
            worker_snapshot.agent_identity.as_deref(),
            Some("agent_worker")
        );
        let ownership = worker_snapshot.agent_ownership.as_ref().unwrap();
        assert_eq!(ownership.origin.agent_id, "agent_lead");
        let current = ownership.current.as_ref().unwrap();
        // The owner's session identity is embedded at capture time so the
        // reference survives owner-session resumes across restarts.
        let session = current.session.as_ref().unwrap();
        assert_eq!(session.value, "lead-session");

        // And the whole thing round-trips through JSON.
        let json = serde_json::to_string(&snapshot).unwrap();
        let restored = parse_snapshot(&json).unwrap();
        let restored_pane = restored.workspaces[1].tabs[0]
            .panes
            .get(&worker_pane.raw())
            .unwrap();
        assert_eq!(
            restored_pane
                .agent_ownership
                .as_ref()
                .unwrap()
                .origin
                .agent_id,
            "agent_lead"
        );
    }

    #[test]
    fn capture_contract_tracks_worktree_space_membership() {
        let mut state = state_with_workspaces(&["main"]);
        state.workspaces[0].worktree_space = Some(crate::workspace::WorktreeSpaceMembership {
            key: "repo-key".into(),
            label: "herdr".into(),
            repo_root: PathBuf::from("/repo/herdr"),
            checkout_path: PathBuf::from("/repo/herdr/worktree-a"),
            is_linked_worktree: true,
        });

        let snapshot = capture_from_state(&state);

        assert_eq!(
            snapshot.workspaces[0].worktree_space,
            state.workspaces[0].worktree_space
        );
    }

    #[test]
    fn capture_contract_tracks_layout_focus_zoom_and_root_pane() {
        let mut state = state_with_workspaces(&["one"]);
        let root = state.workspaces[0].tabs[0].root_pane;
        let second = state.workspaces[0].test_split(Direction::Horizontal);
        state.workspaces[0].tabs[0].layout.focus_pane(second);
        state.toggle_zoom();

        let snapshot = capture_from_state(&state);
        let tab = &snapshot.workspaces[0].tabs[0];
        assert!(matches!(tab.layout, LayoutSnapshot::Split { .. }));
        assert_eq!(tab.focused, Some(second.raw()));
        assert_eq!(tab.root_pane, Some(root.raw()));
        assert!(tab.zoomed);
        assert_eq!(tab.panes.len(), 2);
    }

    #[test]
    fn capture_contract_tracks_focus_navigation() {
        let mut state = state_with_workspaces(&["one"]);
        let root = state.workspaces[0].tabs[0].root_pane;
        let second = state.workspaces[0].test_split(Direction::Horizontal);
        crate::ui::compute_view(&mut state, Rect::new(0, 0, 106, 20));

        state.navigate_pane(NavDirection::Right);

        let snapshot = capture_from_state(&state);
        assert_eq!(snapshot.workspaces[0].tabs[0].focused, Some(second.raw()));
        assert_ne!(snapshot.workspaces[0].tabs[0].focused, Some(root.raw()));
    }

    #[test]
    fn capture_contract_tracks_resize_ratio_changes() {
        let mut state = state_with_workspaces(&["one"]);
        let root = state.workspaces[0].tabs[0].root_pane;
        state.workspaces[0].test_split(Direction::Horizontal);
        state.workspaces[0].layout.focus_pane(root);
        crate::ui::compute_view(&mut state, Rect::new(0, 0, 106, 20));
        let before = capture_from_state(&state);

        state.resize_pane(NavDirection::Right);

        let after = capture_from_state(&state);
        let before_ratio = root_split_ratio(&before.workspaces[0].tabs[0]).unwrap();
        let after_ratio = root_split_ratio(&after.workspaces[0].tabs[0]).unwrap();
        assert_ne!(before_ratio, after_ratio);
    }

    #[test]
    fn capture_contract_tracks_tab_closure() {
        let mut state = state_with_workspaces(&["one"]);
        let second_tab = state.workspaces[0].test_add_tab(Some("logs"));
        state.switch_tab(second_tab);

        state.close_tab();

        let snapshot = capture_from_state(&state);
        let workspace = &snapshot.workspaces[0];
        assert_eq!(workspace.tabs.len(), 1);
        assert_eq!(workspace.active_tab, 0);
        assert!(workspace.tabs[0].custom_name.is_none());
    }

    #[test]
    fn capture_contract_tracks_pane_closure() {
        let mut state = state_with_workspaces(&["one"]);
        state.workspaces[0].test_split(Direction::Horizontal);

        state.close_pane();

        let snapshot = capture_from_state(&state);
        let tab = &snapshot.workspaces[0].tabs[0];
        assert_eq!(tab.panes.len(), 1);
        assert!(matches!(tab.layout, LayoutSnapshot::Pane(_)));
        assert!(!tab.zoomed);
    }

    #[test]
    fn capture_contract_tracks_public_id_counters() {
        let mut state = state_with_workspaces(&["one"]);
        let second = state.workspaces[0].test_split(Direction::Horizontal);
        let third = state.workspaces[0].test_split(Direction::Vertical);
        let second_tab = state.workspaces[0].test_add_tab(None);

        state.workspaces[0].close_pane(second);

        let snapshot = capture_from_state(&state);
        let workspace = &snapshot.workspaces[0];
        assert_eq!(
            workspace.public_pane_numbers,
            HashMap::from([
                (state.workspaces[0].tabs[0].root_pane.raw(), 1),
                (third.raw(), 3),
                (state.workspaces[0].tabs[second_tab].root_pane.raw(), 4),
            ])
        );
        assert_eq!(workspace.next_public_pane_number, 5);
        assert_eq!(workspace.public_tab_numbers, vec![1, 2]);
        assert_eq!(workspace.next_public_tab_number, 3);
    }

    #[test]
    fn capture_contract_tracks_workspace_identity_and_pane_cwds() {
        let mut state = state_with_workspaces(&["one"]);
        let root = state.workspaces[0].tabs[0].root_pane;
        state.workspaces[0].identity_cwd = PathBuf::from("/tmp/pion");
        let second = state.workspaces[0].test_split(Direction::Horizontal);
        state.ensure_test_terminals();
        let root_terminal_id = state.workspaces[0].tabs[0].panes[&root]
            .attached_terminal_id
            .clone();
        state.terminals.get_mut(&root_terminal_id).unwrap().cwd = PathBuf::from("/tmp/pion");
        let second_terminal_id = state.workspaces[0].tabs[0].panes[&second]
            .attached_terminal_id
            .clone();
        state.terminals.get_mut(&second_terminal_id).unwrap().cwd = PathBuf::from("/tmp/herdr");

        let snapshot = capture_from_state(&state);
        let workspace = &snapshot.workspaces[0];
        let tab = &workspace.tabs[0];
        assert_eq!(workspace.identity_cwd, PathBuf::from("/tmp/pion"));
        assert_eq!(tab.panes[&root.raw()].cwd, PathBuf::from("/tmp/pion"));
        assert_eq!(tab.panes[&second.raw()].cwd, PathBuf::from("/tmp/herdr"));
    }

    #[tokio::test]
    async fn capture_contract_tracks_pane_history_from_runtime() {
        let state = state_with_workspaces(&["one"]);
        let root = state.workspaces[0].tabs[0].root_pane;
        let terminal_id = state.workspaces[0].tabs[0].panes[&root]
            .attached_terminal_id
            .clone();
        let mut terminal_runtimes = TerminalRuntimeRegistry::new();
        terminal_runtimes.insert(
            terminal_id,
            crate::terminal::TerminalRuntime::test_with_scrollback_bytes(
                20,
                3,
                4096,
                b"alpha\r\nbeta\r\ngamma\r\n",
            ),
        );

        let snapshot = capture_from_state_with_runtimes(&state, &terminal_runtimes);
        let encoded = serde_json::to_string(&snapshot).unwrap();
        assert!(!encoded.contains("alpha"));
        assert!(!encoded.contains("\"history\""));

        let history_snapshot = capture_history_from_state_with_runtimes(&state, &terminal_runtimes);
        let history = &history_snapshot.workspaces[0].tabs[0].panes[&root.raw()];

        assert!(history.ansi.contains("alpha"));
        assert!(history.ansi.contains("gamma"));
        assert!(history.lines >= 3);
    }

    #[tokio::test]
    async fn capture_contract_tracks_history_for_each_pane() {
        let mut state = state_with_workspaces(&["one"]);
        let first = state.workspaces[0].tabs[0].root_pane;
        let second = state.workspaces[0].test_split(Direction::Horizontal);
        let first_terminal_id = state.workspaces[0].tabs[0].panes[&first]
            .attached_terminal_id
            .clone();
        let second_terminal_id = state.workspaces[0].tabs[0].panes[&second]
            .attached_terminal_id
            .clone();
        let mut terminal_runtimes = TerminalRuntimeRegistry::new();
        terminal_runtimes.insert(
            first_terminal_id,
            crate::terminal::TerminalRuntime::test_with_scrollback_bytes(
                20,
                3,
                4096,
                b"first-pane-history\r\n",
            ),
        );
        terminal_runtimes.insert(
            second_terminal_id,
            crate::terminal::TerminalRuntime::test_with_scrollback_bytes(
                20,
                3,
                4096,
                b"second-pane-history\r\n",
            ),
        );

        let snapshot = capture_from_state_with_runtimes(&state, &terminal_runtimes);
        let encoded = serde_json::to_string(&snapshot).unwrap();
        assert!(!encoded.contains("first-pane-history"));
        assert!(!encoded.contains("second-pane-history"));

        let history_snapshot = capture_history_from_state_with_runtimes(&state, &terminal_runtimes);
        let tab = &history_snapshot.workspaces[0].tabs[0];
        let first_history = &tab.panes[&first.raw()];
        let second_history = &tab.panes[&second.raw()];

        assert!(first_history.ansi.contains("first-pane-history"));
        assert!(second_history.ansi.contains("second-pane-history"));
    }

    #[test]
    fn capture_contract_tracks_hook_authority_agent_session() {
        let mut state = state_with_workspaces(&["one"]);
        let session_path = test_session_path("pi-session.jsonl");
        let root = state.workspaces[0].tabs[0].root_pane;
        state.ensure_test_terminals();
        let terminal_id = state.workspaces[0].tabs[0].panes[&root]
            .attached_terminal_id
            .clone();
        let terminal = state.terminals.get_mut(&terminal_id).unwrap();
        terminal.set_detected_state(
            Some(crate::detect::Agent::Pi),
            crate::detect::AgentState::Idle,
        );
        terminal.set_persisted_agent_session(crate::agent_resume::PersistedAgentSession {
            source: "herdr:pi".into(),
            agent: "pi".into(),
            session_ref: crate::agent_resume::AgentSessionRef::path(session_path.clone()).unwrap(),
        });
        terminal.set_hook_authority_with_session_ref(
            "herdr:pi".into(),
            "pi".into(),
            crate::detect::AgentState::Working,
            None,
            crate::agent_resume::AgentSessionRef::path(session_path.clone()),
            Some(20),
        );

        let snapshot = capture_from_state(&state);
        let agent_session = snapshot.workspaces[0].tabs[0].panes[&root.raw()]
            .agent_session
            .as_ref()
            .expect("agent session should be captured");

        assert_eq!(agent_session.source, "herdr:pi");
        assert_eq!(agent_session.agent, "pi");
        assert_eq!(
            agent_session.kind,
            crate::agent_resume::AgentSessionRefKind::Path
        );
        assert_eq!(agent_session.value, session_path);
    }

    #[test]
    fn capture_contract_preserves_restored_agent_session() {
        let mut state = state_with_workspaces(&["one"]);
        let root = state.workspaces[0].tabs[0].root_pane;
        state.ensure_test_terminals();
        let terminal_id = state.workspaces[0].tabs[0].panes[&root]
            .attached_terminal_id
            .clone();
        state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .set_persisted_agent_session(crate::agent_resume::PersistedAgentSession {
                source: "herdr:opencode".into(),
                agent: "opencode".into(),
                session_ref: crate::agent_resume::AgentSessionRef::id("opencode-session").unwrap(),
            });

        let snapshot = capture_from_state(&state);
        let agent_session = snapshot.workspaces[0].tabs[0].panes[&root.raw()]
            .agent_session
            .as_ref()
            .expect("persisted agent session should be captured");

        assert_eq!(agent_session.source, "herdr:opencode");
        assert_eq!(agent_session.agent, "opencode");
        assert_eq!(
            agent_session.kind,
            crate::agent_resume::AgentSessionRefKind::Id
        );
        assert_eq!(agent_session.value, "opencode-session");
    }

    #[test]
    fn old_unversioned_snapshot_loads_as_version_0() {
        let json = r#"{"workspaces":[],"active":null,"selected":0}"#;
        let snap = parse_snapshot(json).unwrap();
        assert_eq!(snap.version, 0);
    }

    #[test]
    fn future_version_is_rejected() {
        let json = r#"{"version":999,"workspaces":[],"active":null,"selected":0}"#;
        assert!(parse_snapshot(json).is_err());
    }

    #[test]
    fn active_tab_default_is_zero() {
        let json = r#"{"custom_name":"test","identity_cwd":"/tmp","tabs":[]}"#;
        let ws: WorkspaceSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(ws.active_tab, 0);
    }

    #[test]
    fn restore_falls_back_to_home_when_cwd_missing() {
        let mut panes = HashMap::new();
        panes.insert(
            0,
            PaneSnapshot {
                terminal_title: None,
                cwd: PathBuf::from("/tmp/this-directory-does-not-exist-for-herdr-test"),
                label: None,
                agent_name: None,
                managed_agent_kind: None,
                agent_session: None,
                launch_argv: None,
                agent_identity: None,
                agent_ownership: None,
                profiles: Vec::new(),
            },
        );
        panes.insert(
            1,
            PaneSnapshot {
                terminal_title: None,
                cwd: std::env::var("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from("/tmp")),
                label: None,
                agent_name: None,
                managed_agent_kind: None,
                agent_session: None,
                launch_argv: None,
                agent_identity: None,
                agent_ownership: None,
                profiles: Vec::new(),
            },
        );

        let snap = SessionSnapshot {
            version: SNAPSHOT_VERSION,
            workspaces: vec![WorkspaceSnapshot {
                id: Some("test-ws".to_string()),
                custom_name: Some("fallback test".to_string()),
                profiles: Vec::new(),
                identity_cwd: PathBuf::from("/tmp"),
                worktree_space: None,
                public_pane_numbers: HashMap::new(),
                next_public_pane_number: 0,
                public_tab_numbers: Vec::new(),
                next_public_tab_number: 0,
                tabs: vec![TabSnapshot {
                    custom_name: None,
                    layout: LayoutSnapshot::Split {
                        direction: DirectionSnapshot::Horizontal,
                        ratio: 0.5,
                        first: Box::new(LayoutSnapshot::Pane(0)),
                        second: Box::new(LayoutSnapshot::Pane(1)),
                    },
                    panes,
                    zoomed: false,
                    focused: Some(0),
                    root_pane: Some(0),
                }],
                active_tab: 0,
                orchestrator_mode: false,
            }],
            active: Some(0),
            active_profile: crate::workspace::DEFAULT_PROFILE.to_string(),
            selected: 0,
            sidebar_width: Some(26),
            sidebar_section_split: Some(0.5),
            collapsed_space_keys: std::collections::HashSet::new(),
            automations_expanded: false,
            collapsed_agent_group_keys: std::collections::HashSet::new(),
            tree_show_spaces: true,
            tree_show_tabs: true,
            tree_show_agents: true,
            tree_collapsed_spaces: std::collections::HashSet::new(),
            tree_collapsed_tabs: std::collections::HashSet::new(),
            tree_pinned_spaces: std::collections::HashSet::new(),
        };

        let json = serde_json::to_string(&snap).unwrap();
        let restored = parse_snapshot(&json).unwrap();
        assert_eq!(restored.workspaces.len(), 1);
        assert_eq!(
            restored.workspaces[0].tabs[0].panes[&0].cwd,
            PathBuf::from("/tmp/this-directory-does-not-exist-for-herdr-test")
        );
    }
}
