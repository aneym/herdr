//! The unified space → tab → agent sidebar tree.
//!
//! The fork drew this tree server-side from `AppState`; 0.9 renders the whole
//! shell in the client, so the grouping is rebuilt here over
//! [`ClientShellSnapshot`] and the per-client chrome state in
//! [`ClientTreeChrome`]. The shape, the collapse semantics and the hidden
//! section all match `docs/fork/port-0.9/orig/src/ui/sidebar.rs`.

use super::agent_sidebar::AgentRow;
use super::*;

/// Whether a child that needs action unfolds its lane. Off (Alex,
/// 2026-10-05: "default collapse everything workflows so we only see
/// talking agent"); the lane header carries the child's `!` instead.
const ACTION_UNFOLDS_LANE: bool = false;

/// Per-endpoint tree chrome. Defaults show every layer with nothing folded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ClientTreeChrome {
    pub(super) show_spaces: bool,
    pub(super) show_tabs: bool,
    pub(super) show_agents: bool,
    pub(super) collapsed_spaces: HashSet<String>,
    pub(super) collapsed_tabs: HashSet<String>,
    pub(super) pinned_spaces: HashSet<String>,
    pub(super) show_hidden_spaces: bool,
    pub(super) hidden_spaces_expanded: bool,
    pub(super) automations_expanded: bool,
    pub(super) collapsed_agent_groups: HashSet<String>,
    /// Workspace ids in the order the tree lists their spaces. Ids missing from
    /// this list keep their snapshot order behind the ones named here.
    pub(super) space_order: Vec<String>,
    pub(super) factory_expanded_lanes: HashSet<String>,
    pub(super) factory_collapsed_lanes: HashSet<String>,
    pub(super) factory_background_expanded: HashSet<String>,
    pub(super) factory_auto_expanded: HashSet<String>,
    pub(super) factory_parked_expanded: HashSet<String>,
    pub(super) factory_idle_expanded: HashSet<String>,
    pub(super) factory_sections_collapsed: HashSet<String>,
    pub(super) factory_goal_filter: Option<String>,
    pub(super) factory_section_focus: HashMap<String, String>,
}

impl Default for ClientTreeChrome {
    fn default() -> Self {
        Self {
            show_spaces: true,
            show_tabs: true,
            show_agents: true,
            collapsed_spaces: HashSet::new(),
            collapsed_tabs: HashSet::new(),
            pinned_spaces: HashSet::new(),
            show_hidden_spaces: true,
            hidden_spaces_expanded: false,
            automations_expanded: false,
            collapsed_agent_groups: HashSet::new(),
            space_order: Vec::new(),
            factory_expanded_lanes: HashSet::new(),
            factory_collapsed_lanes: HashSet::new(),
            factory_background_expanded: HashSet::new(),
            factory_auto_expanded: HashSet::new(),
            factory_parked_expanded: HashSet::new(),
            factory_idle_expanded: HashSet::new(),
            factory_sections_collapsed: HashSet::new(),
            factory_goal_filter: None,
            factory_section_focus: HashMap::new(),
        }
    }
}

fn valid_factory_goal(value: &str) -> bool {
    let (goal, area) = value.split_once(':').map_or((value, None), |(goal, area)| (goal, Some(area)));
    matches!(goal, "recruiter" | "closer" | "rails") && area.is_none_or(|area| !area.is_empty())
}

pub(super) fn factory_goal_choices(overlay: &crate::factory_overlay::FactoryOverlay) -> Vec<String> {
    let mut choices = Vec::new();
    for goal in ["recruiter", "closer", "rails"] {
        let tags = overlay.tabs.values().filter(|tag| tag.goal.as_deref() == Some(goal)).collect::<Vec<_>>();
        if tags.is_empty() { continue; }
        choices.push(goal.to_owned());
        let areas = tags.iter().filter_map(|tag| tag.goal_area.as_deref()).filter(|area| !area.is_empty())
            .collect::<std::collections::BTreeSet<_>>();
        choices.extend(areas.into_iter().map(|area| format!("{goal}:{area}")));
    }
    choices
}

fn canonical_factory_section(section: &str) -> &str {
    if section == "REVIEWING" { "READY FOR REVIEW" } else { section }
}

fn valid_factory_section(section: &str) -> bool {
    matches!(section, "ORCHESTRATOR" | "READY FOR REVIEW" | "SCOPING" | "IMPLEMENTING" | "MONITORING")
}

fn sorted(values: &HashSet<String>) -> Vec<String> {
    let mut values = values.iter().cloned().collect::<Vec<_>>();
    values.sort();
    values
}

impl ClientTreeChrome {
    pub(super) fn from_preferences(saved: preferences::ClientTreeChromePreferences) -> Self {
        Self {
            show_spaces: saved.show_spaces,
            show_tabs: saved.show_tabs,
            show_agents: saved.show_agents,
            collapsed_spaces: saved.collapsed_spaces.into_iter().collect(),
            collapsed_tabs: saved.collapsed_tabs.into_iter().collect(),
            pinned_spaces: saved.pinned_spaces.into_iter().collect(),
            show_hidden_spaces: saved.show_hidden_spaces,
            hidden_spaces_expanded: saved.hidden_spaces_expanded,
            automations_expanded: saved.automations_expanded,
            collapsed_agent_groups: saved.collapsed_agent_groups.into_iter().collect(),
            space_order: saved.space_order,
            factory_expanded_lanes: saved.factory_expanded_lanes.into_iter().collect(),
            factory_collapsed_lanes: saved.factory_collapsed_lanes.into_iter().collect(),
            factory_background_expanded: saved.factory_background_expanded.into_iter().collect(),
            factory_auto_expanded: saved.factory_auto_expanded.into_iter().collect(),
            factory_parked_expanded: saved.factory_parked_expanded.into_iter().collect(),
            factory_idle_expanded: saved.factory_idle_expanded.into_iter().collect(),
            factory_sections_collapsed: saved.factory_sections_collapsed.into_iter()
                .map(|key| match key.rsplit_once(':') {
                    Some((space, section)) => format!("{space}:{}", canonical_factory_section(section)),
                    None => key,
                })
                .filter(|key| key.rsplit_once(':').is_some_and(|(space, section)| !space.is_empty() && valid_factory_section(section))).collect(),
            factory_goal_filter: saved.factory_goal_filter.filter(|value| valid_factory_goal(value)),
            factory_section_focus: saved.factory_section_focus.into_iter()
                .map(|(space, section)| (space, canonical_factory_section(&section).to_owned()))
                .filter(|(space, section)| !space.is_empty() && valid_factory_section(section)).collect(),
        }
    }

    pub(super) fn to_preferences(&self) -> preferences::ClientTreeChromePreferences {
        preferences::ClientTreeChromePreferences {
            show_spaces: self.show_spaces,
            show_tabs: self.show_tabs,
            show_agents: self.show_agents,
            collapsed_spaces: sorted(&self.collapsed_spaces),
            collapsed_tabs: sorted(&self.collapsed_tabs),
            pinned_spaces: sorted(&self.pinned_spaces),
            show_hidden_spaces: self.show_hidden_spaces,
            hidden_spaces_expanded: self.hidden_spaces_expanded,
            automations_expanded: self.automations_expanded,
            collapsed_agent_groups: sorted(&self.collapsed_agent_groups),
            space_order: self.space_order.clone(),
            factory_expanded_lanes: sorted(&self.factory_expanded_lanes),
            factory_collapsed_lanes: sorted(&self.factory_collapsed_lanes),
            factory_background_expanded: sorted(&self.factory_background_expanded),
            factory_auto_expanded: sorted(&self.factory_auto_expanded),
            factory_parked_expanded: sorted(&self.factory_parked_expanded),
            factory_idle_expanded: sorted(&self.factory_idle_expanded),
            factory_sections_collapsed: sorted(&self.factory_sections_collapsed),
            factory_goal_filter: self.factory_goal_filter.clone(),
            factory_section_focus: self.factory_section_focus.clone(),
        }
    }

    /// Lanes start folded. One opens when the user expanded it, the focused tab
    /// sits inside it, or action unfolding is enabled and a child needs action;
    /// a user collapse always wins.
    fn factory_expanded(&self, tab_id: &str, focused: bool, needs_action: bool) -> bool {
        if self.factory_collapsed_lanes.contains(tab_id) {
            false
        } else {
            self.factory_expanded_lanes.contains(tab_id) || focused || (ACTION_UNFOLDS_LANE && needs_action)
        }
    }

    pub(super) fn toggle(set: &mut HashSet<String>, key: String) {
        if !set.remove(&key) {
            set.insert(key);
        }
    }
}

/// Whether a pane is represented by its tab header instead of a child row.
/// The sidebar promotes a tab's sole top-level agent to the header, even
/// when that agent owns workflow tabs or its group is currently folded.
pub(super) fn pane_is_tab_header(
    snapshot: &ClientShellSnapshot,
    tree: &ClientTreeChrome,
    config: &ClientShellConfig,
    pane_id: &str,
) -> bool {
    if !tree.show_tabs || !tree.show_agents {
        return false;
    }
    let Some(pane) = snapshot.panes.iter().find(|pane| pane.pane_id == pane_id) else {
        return false;
    };
    let rows = super::agent_sidebar::agent_rows(snapshot, config, None);
    let (rows, _) = partition_automations(snapshot, config, rows);
    let arranged = arrange_agent_hierarchy_with(snapshot, tree, rows, false);
    let mut roots = arranged.iter().filter(|row| {
        row.workspace_id == pane.workspace_id && row.tab_id == pane.tab_id && row.group.depth == 0
    });
    roots.next().is_some_and(|row| row.pane_id == pane_id) && roots.next().is_none()
}

/// Collapse-set key for a tab. Tab ids are per-boot, so the key is built from
/// the workspace id and the stable tab number the fork persisted.
pub(super) fn tab_key(workspace_id: &str, number: usize) -> String {
    format!("{workspace_id}#{number}")
}

/// A space or tab grouping row in the unified tree view.
#[derive(Clone, Debug)]
pub(super) struct TreeHeader {
    pub(super) workspace_id: String,
    /// `None` on a space header; the tab this header stands for otherwise.
    pub(super) tab_id: Option<String>,
    pub(super) label: String,
    /// Collapse-set key: workspace id, or `<workspace-id>#<tab-number>`.
    pub(super) key: String,
    pub(super) collapsed: bool,
    /// One status per agent this header stands in for, in panel order, so the
    /// header can show a dot per agent instead of a bare count.
    pub(super) child_states: Vec<crate::api::schema::AgentStatus>,
    /// This header has rows underneath it that a collapse would actually hide.
    /// A header with nothing to hide shows no chevron.
    pub(super) collapsible: bool,
    /// Space headers: this space is pinned, so it stays listed even when no
    /// agent rows remain beneath it. Tab headers: this chat is pinned to the
    /// sidebar's pinned section; the flag only colors the row's pin toggle.
    pub(super) pinned: bool,
    pub(super) indent: u8,
    /// This header's workspace or tab holds the focused pane.
    pub(super) active: bool,
    /// When the agents layer is hidden the header stands in for its agent
    /// rows; if one of them owns a group this carries that group's state, so
    /// the header can show the group chevron and `+N` instead of leaving the
    /// group with no control at all.
    pub(super) group: Option<TreeHeaderGroup>,
    /// Only overlay-tagged space headers carry a summary and attention dot.
    pub(super) space_attention: Option<(crate::factory_overlay::Attention, String)>,
    pub(super) factory_space: bool,
}

/// An agent group surfaced on the header that stands in for its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TreeHeaderGroup {
    /// Local collapse-set key of the group.
    pub(super) key: String,
    pub(super) owner_pane_id: String,
    pub(super) expanded: bool,
    pub(super) server_collapsed: bool,
    /// Descendants hidden by the folded group.
    pub(super) hidden_children: usize,
    /// Most demanding status among them; colors the `+N`.
    pub(super) hidden_status: Option<crate::api::schema::AgentStatus>,
}

impl TreeHeaderGroup {
    /// Width of the summary drawn before the chevron: `+N ` while folded,
    /// nothing while open.
    pub(super) fn summary_width(&self) -> usize {
        // No `+N` on a folded header (Alex, 2026-09-28: too loud at the top
        // level). The chevron takes the hidden rows' colour instead.
        0
    }
}

impl TreeHeader {
    /// The header's chevron slot belongs to an agent group only when the
    /// header has nothing of its own to fold, so the two never compete for
    /// the same cell.
    pub(super) fn group_chevron(&self) -> Option<&TreeHeaderGroup> {
        self.group.as_ref().filter(|_| !self.collapsible)
    }
}

/// The group a header should surface for the rows it stands in for: the first
/// row that owns a group.
fn tree_header_group(rows: &[AgentRow]) -> Option<TreeHeaderGroup> {
    rows.iter().find_map(|row| {
        let expanded = row.group.expanded?;
        let key = row.group.group_key.clone()?;
        Some(TreeHeaderGroup {
            key,
            owner_pane_id: row.pane_id.clone(),
            expanded,
            server_collapsed: row.group.server_collapsed,
            hidden_children: row.group.hidden_children,
            hidden_status: row.group.hidden_status,
        })
    })
}

pub(super) enum AgentPanelListEntry {
    Agent(AgentRow),
    /// The header of the automations section, carrying its activity summary.
    AutomationsHeader(AutomationSummary),
    QuietSections { hidden: usize, automations: AutomationSummary },
    /// An agent in a workspace named by `ui.sidebar.automations.workspaces`.
    Automation(AgentRow),
    /// The collapsible section collecting spaces that were folded away. Only
    /// emitted while the hidden reveal is on.
    HiddenSpacesHeader {
        count: usize,
        collapsed: bool,
    },
    /// The `pinned` section label at the very top of the sidebar.
    PinnedChatsHeader,
    /// A pinned chat from any space, in pin order (the Cmd+1..9 order).
    PinnedTab(PinnedTabRow),
    /// Title of a named group of spaces from the overlay's `space_groups`.
    SpaceGroupHeader { name: String },
    SpaceHeader(TreeHeader),
    TabHeader(TreeHeader),
    FactorySection {
        label: &'static str,
        right: String,
        indent: u8,
        controls: Option<FactorySectionControls>,
    },
    FactoryGoalPicker { filter: Option<String>, choices: Vec<String> },
    FactoryShowAll { workspace_id: String, count: usize, alert: bool, indent: u8 },
    FactoryTab(FactoryTabRow),
    FactoryHost {
        name: String,
        summary: Option<String>,
        attention: crate::factory_overlay::Attention,
        indent: u8,
    },
    FactoryBackground {
        kind: FactoryGroupKind,
        workspace_id: String,
        count: usize,
        collapsed: bool,
        indent: u8,
        alert: bool,
        working: bool,
        shortcut: bool,
    },
}

pub(super) struct FactorySectionControls {
    pub(super) workspace_id: String,
    pub(super) collapsed: bool,
    pub(super) focused: bool,
    pub(super) alert: bool,
    pub(super) count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FactoryGroupKind {
    Automations,
    Parked,
    Closed,
    Background,
}

impl FactoryGroupKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Automations => "services",
            Self::Parked => "parked",
            Self::Closed => "closed",
            Self::Background => "background",
        }
    }
}

/// One row in the pinned section: a chat the pin pulled out of its space.
pub(super) struct PinnedTabRow {
    pub(super) workspace_id: String,
    pub(super) tab_id: String,
    pub(super) label: String,
    /// The chat's home space, drawn muted so cross-space pins stay legible.
    pub(super) space_label: String,
    /// The chat's state, worked out as its row in the spaces tree works it
    /// out (`factory_row`), so a pinned row and its space row agree.
    pub(super) status: crate::api::schema::AgentStatus,
    /// A factory lane with nothing to report, drawn with the idle ring.
    pub(super) idle: bool,
    /// A workflow tab: drawn with the workflow marks, not an agent state.
    pub(super) workflow: bool,
    pub(super) done: bool,
    pub(super) failed: bool,
    /// Cmd+1..9 slot this row owns (pin index + 1); zero when beyond 9.
    pub(super) shortcut: usize,
    /// Position among this machine's live pins.
    pub(super) slot: usize,
    pub(super) active: bool,
    /// Another machine's chat: its badge text and style, drawn after the space
    /// label. `None` on this machine's pins.
    pub(super) machine: Option<(String, ratatui::style::Style)>,
}

/// The pinned section, in endpoint pin order. Pins are a shared session fact,
/// so the section renders straight from the snapshot — no client mirror.
pub(super) fn pinned_tab_entries(
    snapshot: &ClientShellSnapshot,
    overlay: Option<&crate::factory_overlay::FactoryOverlay>,
) -> Vec<AgentPanelListEntry> {
    if snapshot.pinned_tabs.is_empty() {
        return Vec::new();
    }
    let mut out = vec![AgentPanelListEntry::PinnedChatsHeader];
    // Count only pins that resolve to a live tab, matching `numbered_tab_ids`,
    // so a row's digit is exactly the Cmd+N that selects it.
    let live = snapshot
        .pinned_tabs
        .iter()
        .filter_map(|pin| {
            snapshot
                .tabs
                .iter()
                .find(|tab| tab.tab_id == pin.tab_id)
                .map(|tab| (pin, tab))
        })
        .collect::<Vec<_>>();
    // A pinned factory lane or orchestrator shows the state its spaces-tree row
    // shows, live-child rollup included, so the two rows never disagree.
    let mut rolled =
        HashMap::<String, HashMap<String, (crate::api::schema::AgentStatus, bool)>>::new();
    if let Some(overlay) = overlay {
        for (_, tab) in &live {
            let parent = overlay.tab(&tab.tab_id).is_some_and(|tag| {
                matches!(
                    tag.kind,
                    crate::factory_overlay::TabKind::Lane
                        | crate::factory_overlay::TabKind::Orchestrator
                )
            });
            if parent && !rolled.contains_key(&tab.workspace_id) {
                rolled.insert(
                    tab.workspace_id.clone(),
                    factory_row_states(snapshot, overlay, &tab.workspace_id),
                );
            }
        }
    }
    for (index, (pin, tab)) in live.into_iter().enumerate() {
        let space_label = snapshot
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == pin.workspace_id)
            .map(|workspace| workspace.label.clone())
            .unwrap_or_else(|| pin.workspace_id.clone());
        let tag = overlay.and_then(|overlay| overlay.tab(&tab.tab_id));
        let status = chat_status(
            tab.work_status,
            snapshot.agents.iter().filter(|agent| agent.tab_id == tab.tab_id).map(|agent| agent.agent_status),
            tab.agent_status,
            tag,
        );
        let (status, idle) = rolled
            .get(&tab.workspace_id)
            .and_then(|states| states.get(&tab.tab_id))
            .copied()
            .unwrap_or((status, lane_is_idle(tag, status, tab.work_status.is_some())));
        out.push(AgentPanelListEntry::PinnedTab(PinnedTabRow {
            workspace_id: pin.workspace_id.clone(),
            tab_id: pin.tab_id.clone(),
            label: tab.label.clone(),
            space_label,
            status,
            idle,
            workflow: tag.is_some_and(|tag| tag.kind == crate::factory_overlay::TabKind::Workflow),
            done: tag.is_some_and(|tag| tag.done),
            failed: tag.is_some_and(|tag| tag.done && tag.attention == crate::factory_overlay::Attention::Act),
            shortcut: if index < 9 { index + 1 } else { 0 },
            slot: index,
            active: snapshot.focused_workspace_id.as_deref() == Some(tab.workspace_id.as_str())
                && tab.focused,
            machine: None,
        }));
    }
    if out.len() == 1 {
        out.clear();
    }
    out
}

/// Status and idle ring of every factory tab row the spaces tree builds for
/// `workspace_id`, after the lane rollup (live runs, child workflows, grouped
/// lanes). Built from the same `append_factory_space` the tree draws from.
fn factory_row_states(
    snapshot: &ClientShellSnapshot,
    overlay: &crate::factory_overlay::FactoryOverlay,
    workspace_id: &str,
) -> HashMap<String, (crate::api::schema::AgentStatus, bool)> {
    // Factory rows read only each agent's tab and status from these rows.
    let rows = snapshot
        .agents
        .iter()
        .filter(|agent| agent.workspace_id == workspace_id)
        .map(|agent| AgentRow {
            pane_id: agent.pane_id.clone(),
            workspace_id: agent.workspace_id.clone(),
            tab_id: agent.tab_id.clone(),
            status: agent.agent_status,
            focused: agent.focused,
            rows: Vec::new(),
            indent: 0,
            owner_pane_id: agent.owner_pane_id.clone(),
            orphaned: agent.orphaned,
            placement: agent.group.clone(),
            group: AgentGroupRender::default(),
        })
        .collect::<Vec<_>>();
    // Every fold open, so a lane inside a folded services, parked, closed or
    // background group, or under a folded parent, still yields its row.
    let mut unfolded = ClientTreeChrome::default();
    for groups in [
        &mut unfolded.factory_auto_expanded,
        &mut unfolded.factory_parked_expanded,
        &mut unfolded.factory_idle_expanded,
        &mut unfolded.factory_background_expanded,
    ] {
        groups.insert(workspace_id.to_owned());
    }
    unfolded.factory_expanded_lanes = snapshot
        .tabs
        .iter()
        .filter(|tab| tab.workspace_id == workspace_id)
        .map(|tab| tab.tab_id.clone())
        .collect();
    let mut entries = Vec::new();
    super::sidebar_report::paused(|| {
        append_factory_space(
            &mut entries,
            snapshot,
            &unfolded,
            workspace_id,
            &rows,
            overlay,
            0,
        )
    });
    let mut states = HashMap::new();
    for entry in entries {
        if let AgentPanelListEntry::FactoryTab(row) = entry {
            if let Some(tab_id) = row.header.tab_id.filter(|tab_id| *tab_id == row.header.key) {
                states.entry(tab_id).or_insert((row.status, row.idle));
            }
        }
    }
    states
}

/// A tagged tab stands in for its agents, while still following the tab-header hit path.
pub(super) struct FactoryTabRow {
    pub(super) header: TreeHeader,
    pub(super) status: crate::api::schema::AgentStatus,
    pub(super) reviewing: bool,
    pub(super) review_url: Option<String>,
    pub(super) scoping: bool,
    pub(super) scope_url: Option<String>,
    pub(super) badge: Option<String>,
    /// Remote machine a pane in this tab runs its foreground job on.
    pub(super) machine: Option<String>,
    pub(super) phase: Option<String>,
    pub(super) started: Option<i64>,
    pub(super) summary: Option<String>,
    pub(super) attention: crate::factory_overlay::Attention,
    /// Derived from this client's live lane status, not the overlay's delayed idle hint.
    pub(super) idle: bool,
    pub(super) idle_reason: Option<String>,
    pub(super) devloop: bool,
    pub(super) background: bool,
    pub(super) workflow: bool,
    pub(super) done: bool,
}

impl AgentPanelListEntry {
    pub(super) fn line_count(&self) -> usize {
        match self {
            Self::Agent(row) | Self::Automation(row) => row.rows.len().max(1),
            Self::FactoryTab(row) if row.workflow && !row.done
                && row.phase.as_ref().is_some_and(|phase| !phase.trim().is_empty()) => 2,
            _ => 1,
        }
    }
}

/// What the automations header reports about the rows it stands for. Long-lived
/// background work should not shout; the header only turns red when something
/// is actually blocked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct AutomationSummary {
    pub(super) blocked: usize,
    pub(super) working: usize,
    pub(super) done: usize,
    pub(super) total: usize,
}

impl AutomationSummary {
    fn of(rows: &[AgentRow]) -> Self {
        use crate::api::schema::AgentStatus;
        let mut summary = Self {
            total: rows.len(),
            ..Self::default()
        };
        for row in rows {
            match row.status {
                AgentStatus::Blocked => summary.blocked += 1,
                AgentStatus::Working => summary.working += 1,
                AgentStatus::Done => summary.done += 1,
                AgentStatus::Idle | AgentStatus::Unknown => {}
            }
        }
        summary
    }

    pub(super) fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.blocked > 0 {
            parts.push(format!("{} blocked", self.blocked));
        }
        if self.working > 0 {
            parts.push(format!("{} working", self.working));
        }
        if self.done > 0 {
            parts.push(format!("{} done", self.done));
        }
        if parts.is_empty() {
            parts.push(self.total.to_string());
        }
        parts.join(" \u{b7} ")
    }

    pub(super) fn color(&self, palette: &Palette) -> ratatui::style::Color {
        if self.blocked > 0 {
            palette.red
        } else {
            palette.overlay0
        }
    }
}

/// Split the panel rows into ordinary agents and automations. A row is an
/// automation when its workspace label is listed in
/// `ui.sidebar.automations.workspaces`.
pub(super) fn partition_automations(
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    rows: Vec<AgentRow>,
) -> (Vec<AgentRow>, Vec<AgentRow>) {
    if config.automations.workspaces.is_empty() {
        return (rows, Vec::new());
    }
    rows.into_iter().partition(|row| {
        !config
            .automations
            .workspaces
            .contains(&workspace_label(snapshot, &row.workspace_id))
    })
}

/// Append the automations section to a finished panel list.
pub(super) fn append_automations(
    entries: &mut Vec<AgentPanelListEntry>,
    tree: &ClientTreeChrome,
    automations: Vec<AgentRow>,
) {
    if automations.is_empty() {
        return;
    }
    let summary = AutomationSummary::of(&automations);
    if !tree.automations_expanded {
        if let Some(AgentPanelListEntry::HiddenSpacesHeader { count, collapsed: true }) = entries.last() {
            let hidden = *count;
            entries.pop();
            entries.push(AgentPanelListEntry::QuietSections { hidden, automations: summary });
            return;
        }
    }
    entries.push(AgentPanelListEntry::AutomationsHeader(summary));
    if tree.automations_expanded {
        entries.extend(automations.into_iter().map(AgentPanelListEntry::Automation));
    }
}

/// The tree view draws the overlay grouping, so `[ui.factory] enabled` turns it on
/// without touching the configured sort.
pub(super) fn tree_view_active(config: &ClientShellConfig) -> bool {
    config.factory.enabled || config.agent_panel_sort == crate::config::AgentPanelSortConfig::Tree
}

/// Cmd+E / tree roll-up ordering. Higher wins. Blocked agents are waiting on
/// Alex, so they come first. An unread completion is the next thing worth
/// reading. A read completion is still visitable. A working agent has nothing
/// to read yet, so it ranks last among live states.
///
/// This is deliberately *not* [`super::status_priority`]: that one ranks a
/// working agent above an idle one for the space roll-up badge, where recency
/// of activity is what matters.
pub(super) fn cycle_attention_rank(status: crate::api::schema::AgentStatus) -> u8 {
    use crate::api::schema::AgentStatus;
    match status {
        AgentStatus::Blocked => 4,
        AgentStatus::Done => 3,
        AgentStatus::Idle => 2,
        AgentStatus::Working => 1,
        AgentStatus::Unknown => 0,
    }
}

/// Highest-attention status among a run of agent rows.
fn rollup_state(rows: &[AgentRow]) -> Vec<crate::api::schema::AgentStatus> {
    rows.iter().map(|row| row.status).collect()
}

/// Group agent rows into space → tab → agent rows, preserving the incoming
/// order so nothing reshuffles on its own. Layer visibility and collapse state
/// come from the three tree layer toggles and the two collapse sets.
#[allow(dead_code)] // The stock tree test and fork callers retain this compatibility entry point.
pub(super) fn tree_list_entries(
    snapshot: &ClientShellSnapshot,
    tree: &ClientTreeChrome,
    rows: Vec<AgentRow>,
) -> Vec<AgentPanelListEntry> {
    tree_list_entries_with_overlay(snapshot, tree, rows, None)
}

pub(super) fn tree_list_entries_with_overlay(
    snapshot: &ClientShellSnapshot,
    tree: &ClientTreeChrome,
    rows: Vec<AgentRow>,
    overlay: Option<&crate::factory_overlay::FactoryOverlay>,
) -> Vec<AgentPanelListEntry> {
    let mut workspace_order = Vec::<String>::new();
    let mut by_workspace = HashMap::<String, Vec<AgentRow>>::new();
    for row in rows {
        let workspace_id = row.workspace_id.clone();
        by_workspace
            .entry(workspace_id.clone())
            .or_insert_with(|| {
                workspace_order.push(workspace_id);
                Vec::new()
            })
            .push(row);
    }

    if let Some(overlay) = overlay {
        // Include agentless tagged spaces and collapsed spaces with tabs, so
        // folding an untagged space does not remove it from the hidden count.
        // Preserve the existing relative order of agent-bearing spaces.
        for (index, workspace) in snapshot.workspaces.iter().enumerate() {
            let id = &workspace.workspace_id;
            let collapsed_with_tabs = workspace.visible_in_profile
                && tree.show_spaces
                && tree.show_hidden_spaces
                && tree.collapsed_spaces.contains(id)
                && snapshot.tabs.iter().any(|tab| &tab.workspace_id == id);
            if workspace_order.contains(id)
                || !(collapsed_with_tabs || overlay.space_is_tagged(
                    snapshot
                        .tabs
                        .iter()
                        .filter(|tab| &tab.workspace_id == id)
                        .map(|tab| tab.tab_id.as_str()),
                ))
            {
                continue;
            }
            let before = snapshot.workspaces[index + 1..]
                .iter()
                .filter_map(|next| {
                    workspace_order
                        .iter()
                        .position(|present| present == &next.workspace_id)
                })
                .next()
                .unwrap_or(workspace_order.len());
            workspace_order.insert(before, id.clone());
        }
    }
    let mut out = pinned_tab_entries(snapshot, overlay);
    if let Some(overlay) = overlay.filter(|overlay| overlay.tabs.values().any(|tag| tag.section.is_some())) {
        let choices = factory_goal_choices(overlay);
        if !choices.is_empty() {
            out.push(AgentPanelListEntry::FactoryGoalPicker {
                filter: tree.factory_goal_filter.clone().filter(|value| choices.contains(value)), choices,
            });
        }
    }
    // Collapsed spaces move out of their slot and collect under one collapsible
    // section at the bottom, so folding a space away actually clears the row it
    // occupied instead of leaving a stub mid-tree.
    let mut hidden_out = Vec::<AgentPanelListEntry>::new();
    let mut hidden_spaces = HashSet::<String>::new();
    for workspace_id in &workspace_order {
        let workspace_rows = by_workspace.remove(workspace_id).unwrap_or_default();
        let tagged = overlay.filter(|overlay| {
            overlay.space_is_tagged(
                snapshot
                    .tabs
                    .iter()
                    .filter(|tab| &tab.workspace_id == workspace_id)
                    .map(|tab| tab.tab_id.as_str()),
            )
        });
        let space_collapsed = tree.show_spaces && tree.collapsed_spaces.contains(workspace_id);
        let demoted = space_collapsed && tree.show_hidden_spaces;
        if demoted {
            hidden_spaces.insert(workspace_id.clone());
        }
        let out = if demoted { &mut hidden_out } else { &mut out };
        let space_indent = u8::from(tree.show_spaces);
        if tree.show_spaces {
            // A collapsed space is deliberately folded out of sight; its status
            // dots would keep pulling attention to it, so they hide with the
            // rows. Dots on an expanded header still stand in for agents hidden
            // by the layer toggles.
            let layers_hidden = !tree.show_tabs && !tree.show_agents;
            let show_dots = !space_collapsed && layers_hidden;
            out.push(AgentPanelListEntry::SpaceHeader(TreeHeader {
                workspace_id: workspace_id.clone(),
                tab_id: None,
                label: workspace_label(snapshot, workspace_id),
                key: workspace_id.clone(),
                collapsed: space_collapsed,
                child_states: if show_dots {
                    rollup_state(&workspace_rows)
                } else {
                    Vec::new()
                },
                collapsible: (!workspace_rows.is_empty() || tagged.is_some())
                    && (tree.show_tabs || tree.show_agents),
                pinned: tree.pinned_spaces.contains(workspace_id),
                indent: 0,
                // A space row separates groups; it is never the selection.
                // Highlighting it while a tab inside it is selected reads as two
                // things being active at once.
                active: false,
                // Stands in for its agents whenever the layers that would show
                // them are off, collapsed or not: a collapsed space with nothing
                // beneath it still needs the group control.
                group: layers_hidden
                    .then(|| tree_header_group(&workspace_rows))
                    .flatten(),
                space_attention: overlay.and_then(|overlay| {
                    space_attention(snapshot, overlay, workspace_id)
                }),
                factory_space: tagged.is_some(),
            }));
            if space_collapsed {
                if let Some(overlay) = tagged {
                    if super::sidebar_report::recording() && snapshot.tabs.iter().any(|tab|
                        tab.workspace_id == *workspace_id && overlay.tab(&tab.tab_id).is_some_and(|tag| tag.section.is_some())) {
                        let mut report = super::sidebar_report::WorkspaceReport::new(workspace_id, tree, None);
                        for tab in snapshot.tabs.iter().filter(|tab| tab.workspace_id == *workspace_id) {
                            if let Some(tag) = overlay.tab(&tab.tab_id) {
                                report.add(&tab.tab_id, tag.kind,
                                    super::sidebar_report::section_label(tag.section).map(str::to_owned),
                                    tag.parent.clone(), "space_collapsed");
                            }
                        }
                        super::sidebar_report::record(report);
                    }
                }
                continue;
            }
        }

        if let Some(overlay) = tagged {
            append_factory_space(
                out,
                snapshot,
                tree,
                workspace_id,
                &workspace_rows,
                overlay,
                space_indent,
            );
            continue;
        }

        let mut tab_order = Vec::<String>::new();
        let mut by_tab = HashMap::<String, Vec<AgentRow>>::new();
        // An owned agent sits under its owner's tab, not its own: a workflow a
        // lane spawned in a separate tab nests under the lane instead of opening
        // a tab header of its own. Rows arrive depth-first, so the tab each
        // depth resolved to is the parent's tab for the next depth. The tree
        // with tabs shown arranges without orchestrator adoption, so every
        // parent edge here is a real owner or an explicit `under`.
        let mut tab_at_depth = Vec::<String>::new();
        for row in workspace_rows {
            let depth = usize::from(row.group.depth);
            tab_at_depth.truncate(depth);
            let tab_id = match depth.checked_sub(1).and_then(|d| tab_at_depth.get(d)) {
                Some(parent_tab) => parent_tab.clone(),
                None => row.tab_id.clone(),
            };
            tab_at_depth.push(tab_id.clone());
            by_tab
                .entry(tab_id.clone())
                .or_insert_with(|| {
                    tab_order.push(tab_id);
                    Vec::new()
                })
                .push(row);
        }

        for tab_id in &tab_order {
            let Some(mut tab_rows) = by_tab.remove(tab_id) else {
                continue;
            };
            let mut agent_indent = space_indent;
            // A tab led by one agent (a lane chat, with whatever it spawned
            // beneath it) is that chat: the header row stands for the agent,
            // carrying its state dot and its group chevron, and the agent's
            // own row is dropped so the chat is not listed twice.
            let lead_depth = tab_rows
                .first()
                .map(|row| row.group.depth)
                .filter(|_| tree.show_tabs && tree.show_agents)
                .filter(|depth| tab_rows[1..].iter().all(|row| row.group.depth > *depth));
            let lead = lead_depth.map(|depth| (tab_rows.remove(0), depth));
            if tree.show_tabs {
                let tab = snapshot.tabs.iter().find(|tab| &tab.tab_id == tab_id);
                let key = tab
                    .map(|tab| tab_key(workspace_id, tab.number))
                    .unwrap_or_else(|| tab_key(workspace_id, 0));
                let collapsed = lead.is_none() && tree.collapsed_tabs.contains(&key);
                let show_dots = collapsed || !tree.show_agents || lead.is_some();
                out.push(AgentPanelListEntry::TabHeader(TreeHeader {
                    workspace_id: workspace_id.clone(),
                    tab_id: Some(tab_id.clone()),
                    label: tab
                        .map(|tab| tab.label.clone())
                        .unwrap_or_else(|| tab_id.clone()),
                    key,
                    collapsed,
                    child_states: match &lead {
                        Some((row, _)) => vec![row.status],
                        None if show_dots => rollup_state(&tab_rows),
                        None => Vec::new(),
                    },
                    collapsible: lead.is_none() && !tab_rows.is_empty() && tree.show_agents,
                    pinned: snapshot
                        .pinned_tabs
                        .iter()
                        .any(|pin| pin.tab_id == *tab_id),
                    indent: space_indent,
                    active: (!tree.show_agents || lead.is_some())
                        && tab.is_some_and(|tab| tab.focused)
                        && snapshot.focused_workspace_id.as_deref() == Some(workspace_id.as_str()),
                    // A tab whose agent rows are hidden stands in for them, even
                    // when the tab itself was collapsed earlier: that collapse
                    // hides nothing now, and a stale key must not swallow the
                    // only control the group has.
                    group: match &lead {
                        Some((row, _)) => tree_header_group(std::slice::from_ref(row)),
                        None => (!tree.show_agents)
                            .then(|| tree_header_group(&tab_rows))
                            .flatten(),
                    },
                    space_attention: None,
                    factory_space: false,
                }));
                if collapsed {
                    continue;
                }
                agent_indent = agent_indent.saturating_add(1);
            }

            if !tree.show_agents {
                continue;
            }

            for row in &mut tab_rows {
                row.indent = agent_indent;
                if let Some((_, lead_depth)) = &lead {
                    // Under a header that stands for the chat, the chat's
                    // children hang off the header's own column.
                    row.indent = space_indent;
                    row.group.depth = row.group.depth.saturating_sub(*lead_depth);
                }
                // The headers already name the space and tab; drop the duplicate
                // labels from the row tokens.
                row.strip_tokens(tree.show_spaces, tree.show_tabs);
            }
            out.extend(tab_rows.into_iter().map(AgentPanelListEntry::Agent));
        }
    }

    // Pinned spaces stay listed even when nothing runs in them, so a space keeps
    // its header (and its new-tab plus) instead of vanishing when its last agent
    // goes away. They follow the agent-bearing spaces, in workspace order.
    if tree.show_spaces && !tree.pinned_spaces.is_empty() {
        let listed = out
            .iter()
            .chain(hidden_out.iter())
            .filter_map(|entry| match entry {
                AgentPanelListEntry::SpaceHeader(header) => Some(header.workspace_id.clone()),
                _ => None,
            })
            .collect::<HashSet<_>>();
        for workspace in &snapshot.workspaces {
            let workspace_id = &workspace.workspace_id;
            if !workspace.visible_in_profile
                || listed.contains(workspace_id)
                || !tree.pinned_spaces.contains(workspace_id)
            {
                continue;
            }
            let collapsed = tree.collapsed_spaces.contains(workspace_id);
            let header = AgentPanelListEntry::SpaceHeader(TreeHeader {
                workspace_id: workspace_id.clone(),
                tab_id: None,
                label: workspace.label.clone(),
                key: workspace_id.clone(),
                collapsed,
                child_states: Vec::new(),
                collapsible: false,
                pinned: true,
                indent: 0,
                active: false,
                group: None,
                space_attention: overlay.and_then(|overlay| {
                    space_attention(snapshot, overlay, workspace_id)
                }),
                factory_space: false,
            });
            // A pinned space carries no agent rows, so collapsing it hides
            // nothing on its own; the section is where it goes to get out of the
            // way.
            if collapsed && tree.show_hidden_spaces {
                hidden_spaces.insert(workspace_id.clone());
                hidden_out.push(header);
            } else {
                out.push(header);
            }
        }
    }

    // The manual order applies to the visible tree only; a space inside the
    // hidden section stays inside it.
    let mut out = group_spaces(reorder_spaces(out, &tree.space_order), overlay);

    // One collapsible section carries every collapsed space, so the tree above
    // it holds only what is still meant to be seen.
    if !hidden_out.is_empty() {
        let collapsed = !tree.hidden_spaces_expanded;
        out.push(AgentPanelListEntry::HiddenSpacesHeader {
            count: hidden_spaces.len(),
            collapsed,
        });
        if !collapsed {
            out.append(&mut reorder_spaces(hidden_out, &tree.space_order));
        }
    }
    // Pinned chats own the first Cmd slots, so a section's "⌘1..9" hint moves
    // past them (or goes away once pins fill all nine).
    let pins = out
        .iter()
        .filter(|entry| matches!(entry, AgentPanelListEntry::PinnedTab(_)))
        .count();
    if pins > 0 {
        let shifted = match pins {
            0..=7 => format!("⌘{}..9", pins + 1),
            8 => "⌘9".to_owned(),
            _ => String::new(),
        };
        for entry in &mut out {
            if let AgentPanelListEntry::FactorySection { right, .. } = entry {
                *right = right.replace("⌘1..9", &shifted).trim().to_string();
            }
        }
    }
    out
}

/// The dot a space row shows: the worst of the space's own tag and the attention of
/// its tagged tabs, so a writer that only tags tabs still lights the space row.
/// The summary comes from the space tag. Untagged spaces return `None`.
fn space_attention(
    snapshot: &ClientShellSnapshot,
    overlay: &crate::factory_overlay::FactoryOverlay,
    workspace_id: &str,
) -> Option<(crate::factory_overlay::Attention, String)> {
    let tag = overlay.space(workspace_id);
    let rolled = snapshot
        .tabs
        .iter()
        .filter(|tab| tab.workspace_id == workspace_id)
        .filter_map(|tab| overlay.tab(&tab.tab_id))
        .map(|tag| tag.attention)
        .max_by_key(|attention| attention.rank());
    let worst = [tag.map(|tag| tag.attention), rolled]
        .into_iter()
        .flatten()
        .max_by_key(|attention| attention.rank())?;
    (worst != crate::factory_overlay::Attention::None).then(|| {
        (
            worst,
            tag.and_then(|tag| tag.summary.clone()).unwrap_or_default(),
        )
    })
}

/// Group a tagged space in tab order. Only unknown/untagged tabs keep agent rows.
fn append_factory_space(
    out: &mut Vec<AgentPanelListEntry>,
    snapshot: &ClientShellSnapshot,
    tree: &ClientTreeChrome,
    workspace_id: &str,
    rows: &[AgentRow],
    overlay: &crate::factory_overlay::FactoryOverlay,
    indent: u8,
) {
    use crate::factory_overlay::{TabKind, TabMode, TabSection};
    let start = out.len();
    let sectioned = snapshot.tabs.iter().filter(|tab| tab.workspace_id == workspace_id)
        .any(|tab| overlay.tab(&tab.tab_id).is_some_and(|tag| tag.section.is_some()));
    let choices = factory_goal_choices(overlay);
    let filter = tree.factory_goal_filter.as_deref().filter(|value| sectioned
        && !tree.collapsed_spaces.contains(workspace_id) && choices.iter().any(|choice| choice == value));
    let tabs = snapshot
        .tabs
        .iter()
        .filter(|tab| tab.workspace_id == workspace_id)
        .filter(|tab| filter.is_none_or(|filter| overlay.tab(&tab.tab_id).is_some_and(|tag| {
            let (goal, area) = filter.split_once(':').map_or((filter, None), |(goal, area)| (goal, Some(area)));
            tag.kind == TabKind::Orchestrator || tag.mode == TabMode::Auto
                || (tag.goal.as_deref() == Some(goal) && area.is_none_or(|area| tag.goal_area.as_deref() == Some(area)))
        })))
        .collect::<Vec<_>>();
    let kind = |tab: &crate::protocol::ClientShellTab| {
        overlay
            .tab(&tab.tab_id)
            .map_or(TabKind::Unknown, |tag| tag.kind)
    };
    let foreground = |tab: &&crate::protocol::ClientShellTab| {
        overlay
            .tab(&tab.tab_id)
            .is_none_or(|tag| !tag.done && tag.kind != TabKind::Advisor)
    };
    let orchestrators = tabs
        .iter()
        .copied()
        .filter(|tab| foreground(tab) && kind(tab) == TabKind::Orchestrator)
        .collect::<Vec<_>>();
    let lanes = tabs
        .iter()
        .copied()
        .filter(|tab| kind(tab) == TabKind::Lane && (foreground(tab)
            || overlay.tab(&tab.tab_id).is_some_and(|tag| tag.mode != TabMode::Active)))
        .collect::<Vec<_>>();
    let lane_mode = |tab: &crate::protocol::ClientShellTab| {
        overlay.tab(&tab.tab_id).map_or(TabMode::Active, |tag| tag.mode)
    };
    let all_workflows = tabs
        .iter()
        .copied()
        .filter(|tab| kind(tab) == TabKind::Workflow)
        .collect::<Vec<_>>();
    let workflows = all_workflows
        .iter()
        .copied()
        .filter(|tab| overlay.tab(&tab.tab_id).is_none_or(|tag| !tag.done))
        .collect::<Vec<_>>();
    let first_orchestrator = orchestrators.first().map(|tab| tab.tab_id.as_str());
    let lane_ids = lanes
        .iter()
        .map(|tab| tab.tab_id.as_str())
        .collect::<HashSet<_>>();
    let parent_for = |tab: &crate::protocol::ClientShellTab| -> Option<&str> {
        let parent = overlay.tab(&tab.tab_id)?.parent.as_deref();
        if parent.is_some_and(|id| lane_ids.contains(id)) {
            parent
        } else {
            first_orchestrator
        }
    };
    // Only a tab's first agent pane determines its explicit sidebar placement.
    // Resolve chains to their top row so the factory tree never grows past one level.
    let direct_parent = lanes.iter().filter(|lane| lane_mode(lane) == TabMode::Active)
        .filter(|lane| overlay.tab(&lane.tab_id).and_then(|tag| tag.section) != Some(TabSection::Scoping))
        .filter_map(|lane| {
            let primary = snapshot.agents.iter().find(|agent| agent.tab_id == lane.tab_id)?;
            let pane = primary.group.parent_pane_id.as_deref()?;
            let parent = snapshot.agents.iter().find(|agent| agent.pane_id == pane)?;
            if parent.workspace_id != workspace_id || parent.tab_id == lane.tab_id {
                return None;
            }
            let valid = lanes.iter().any(|candidate| candidate.tab_id == parent.tab_id)
                || orchestrators.iter().any(|candidate| candidate.tab_id == parent.tab_id);
            valid.then_some((lane.tab_id.as_str(), parent.tab_id.as_str()))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let grouped_root = |tab_id: &str| {
        let mut seen = HashSet::new();
        let mut current = tab_id;
        while let Some(&parent) = direct_parent.get(current) {
            if !seen.insert(current) || seen.contains(parent) {
                return None; // A cycle is not a sidebar hierarchy.
            }
            current = parent;
        }
        (current != tab_id).then(|| current.to_owned())
    };
    let lane_section = |lane: &crate::protocol::ClientShellTab| {
        overlay.tab(&lane.tab_id).and_then(|tag| tag.section).unwrap_or(TabSection::Implementing)
    };
    let group_state = |members: &[&crate::protocol::ClientShellTab]| {
        let member_ids = members.iter().map(|lane| lane.tab_id.as_str()).collect::<HashSet<_>>();
        let group_lanes = lanes.iter().copied().filter(|lane| {
            member_ids.contains(lane.tab_id.as_str())
                || grouped_root(&lane.tab_id).is_some_and(|id| member_ids.contains(id.as_str()))
        }).collect::<Vec<_>>();
        let group_ids = group_lanes.iter().map(|lane| lane.tab_id.as_str()).collect::<HashSet<_>>();
        group_lanes.iter().copied().chain(members.iter().copied().filter(|tab| !group_ids.contains(tab.tab_id.as_str()))).chain(all_workflows.iter().copied().filter(|workflow| {
            parent_for(workflow).is_some_and(|id| group_ids.contains(id))
        })).fold((false, false), |(alert, working), tab| {
            let row = factory_row(snapshot, rows, overlay, tab, indent, false, false);
            let (row_alert, row_working) = match row {
                AgentPanelListEntry::FactoryTab(row) => (
                    row.status == crate::api::schema::AgentStatus::Blocked
                        || row.attention == crate::factory_overlay::Attention::Act,
                    row.status == crate::api::schema::AgentStatus::Working,
                ),
                _ => (false, false),
            };
            let tag = overlay.tab(&tab.tab_id);
            (alert || row_alert || tag.is_some_and(|tag| tag.runs.iter()
                    .any(|run| run.attention == crate::factory_overlay::Attention::Act)),
                working || row_working || tab.work_status.is_none() && tag.is_some_and(|tag| tag.busy
                    || tag.runs.iter().any(|run| !run.done)))
        })
    };
    let orchestrator_lanes = if sectioned {
        lanes.iter().copied().filter(|lane| lane_mode(lane) == TabMode::Active
            && grouped_root(&lane.tab_id).is_none() && lane_section(lane) == TabSection::Orchestrator)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if !orchestrators.is_empty() || !orchestrator_lanes.is_empty() {
        out.push(AgentPanelListEntry::FactorySection {
            label: "ORCHESTRATOR",
            right: "⌘0".to_owned(),
            indent,
            controls: sectioned.then(|| {
                let mut members = orchestrators.clone();
                members.extend_from_slice(&orchestrator_lanes);
                FactorySectionControls { workspace_id: workspace_id.to_owned(), collapsed: false, focused: false,
                    alert: group_state(&members).0, count: members.len() }
            }),
        });
        for orchestrator in &orchestrators {
            if first_orchestrator == Some(orchestrator.tab_id.as_str())
                || lanes.iter().any(|lane| grouped_root(&lane.tab_id).as_deref() == Some(orchestrator.tab_id.as_str())) {
                let children = workflows.iter().copied()
                    .filter(|workflow| parent_for(workflow) == Some(orchestrator.tab_id.as_str()))
                    .collect::<Vec<_>>();
                let focused = snapshot.focused_workspace_id.as_deref() == Some(workspace_id)
                    && children.iter().any(|tab| {
                        snapshot.focused_tab_id.as_deref() == Some(tab.tab_id.as_str())
                    });
                let attention = all_workflows.iter()
                    .filter(|workflow| parent_for(workflow) == Some(orchestrator.tab_id.as_str()))
                    .filter_map(|workflow| overlay.tab(&workflow.tab_id))
                    .map(|tag| tag.attention)
                    .max_by_key(|attention| attention.rank());
                let runs = overlay.tab(&orchestrator.tab_id).map_or(&[][..], |tag| tag.runs.as_slice());
                let grouped = lanes.iter().copied()
                    .filter(|lane| grouped_root(&lane.tab_id).as_deref() == Some(orchestrator.tab_id.as_str()))
                    .collect::<Vec<_>>();
                let grouped_ids = grouped.iter().map(|lane| lane.tab_id.as_str()).collect::<HashSet<_>>();
                let grouped_workflows = workflows.iter().copied()
                    .filter(|workflow| parent_for(workflow).is_some_and(|id| grouped_ids.contains(id)))
                    .collect::<Vec<_>>();
                let (run_agents, run_workflows) = split_runs(orchestrator, runs);
                let (grouped_agents, grouped_runs) = grouped.iter()
                    .filter_map(|lane| overlay.tab(&lane.tab_id).map(|tag| split_runs(lane, &tag.runs)))
                    .fold((0, 0), |(agents, workflows), (a, w)| (agents + a, workflows + w));
                let attention = all_workflows.iter()
                    .filter(|workflow| parent_for(workflow).is_some_and(|id| grouped_ids.contains(id)))
                    .filter_map(|workflow| overlay.tab(&workflow.tab_id))
                    .map(|tag| tag.attention)
                    .chain(grouped.iter().filter_map(|lane| overlay.tab(&lane.tab_id))
                        .map(|tag| tag.attention))
                    .chain(runs.iter().map(|run| run.attention))
                    .chain(grouped.iter().filter_map(|lane| overlay.tab(&lane.tab_id))
                        .flat_map(|tag| tag.runs.iter().map(|run| run.attention)))
                    .chain(attention).max_by_key(|value| value.rank());
                let focused = focused || snapshot.focused_workspace_id.as_deref() == Some(workspace_id)
                    && grouped.iter().chain(grouped_workflows.iter()).any(|tab| {
                        snapshot.focused_tab_id.as_deref() == Some(tab.tab_id.as_str())
                    });
                let agents = grouped.len() + run_agents + grouped_agents;
                let workflow_count = children.len() + grouped_workflows.len() + run_workflows + grouped_runs;
                let running = agents + workflow_count;
                let expanded = tree.factory_expanded(&orchestrator.tab_id, focused,
                    attention == Some(crate::factory_overlay::Attention::Act));
                let mut row = factory_row(snapshot, rows, overlay, orchestrator, indent,
                    !expanded && running > 0, running > 0);
                if let AgentPanelListEntry::FactoryTab(tab) = &mut row {
                    if let Some(attention) = attention.filter(|child| child.rank() > tab.attention.rank()) {
                        tab.attention = attention;
                    }
                }
                summarize_factory_parent(&mut row, agents, workflow_count,
                    overlay.tab(&orchestrator.tab_id).is_some_and(|tag| tag.busy), true,
                    orchestrator.work_status.is_some());
                out.push(row);
                if expanded {
                    for lane in grouped {
                        out.push(factory_row(snapshot, rows, overlay, lane,
                            indent.saturating_add(1), false, false));
                        for workflow in workflows.iter().copied()
                            .filter(|workflow| parent_for(workflow) == Some(lane.tab_id.as_str())) {
                            out.push(factory_row(snapshot, rows, overlay, workflow,
                                indent.saturating_add(2), false, false));
                        }
                        if let Some(tag) = overlay.tab(&lane.tab_id) {
                            for run in &tag.runs {
                                out.push(factory_run_row(lane, run, indent.saturating_add(2)));
                            }
                        }
                    }
                    for child in children {
                        out.push(factory_row(snapshot, rows, overlay, child,
                            indent.saturating_add(1), false, false));
                    }
                    for run in runs {
                        out.push(factory_run_row(orchestrator, run, indent.saturating_add(1)));
                    }
                }
            } else {
                out.push(factory_row(snapshot, rows, overlay, orchestrator, indent, false, false));
            }
        }
    }
    let root_workflows = if first_orchestrator.is_none() {
        workflows
            .iter()
            .copied()
            .filter(|workflow| parent_for(workflow).is_none())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let ordinary = tabs
        .iter()
        .copied()
        .filter(|tab| foreground(tab) && kind(tab) == TabKind::Unknown)
        .collect::<Vec<_>>();
    let push_lane = |out: &mut Vec<AgentPanelListEntry>, lane: &crate::protocol::ClientShellTab, indent: u8, grouped: bool| {
            let children = workflows
                .iter()
                .copied()
                .filter(|workflow| parent_for(workflow) == Some(lane.tab_id.as_str()))
                .collect::<Vec<_>>();
            let grouped_lanes = lanes.iter().copied()
                .filter(|child| grouped_root(&child.tab_id).as_deref() == Some(lane.tab_id.as_str()))
                .collect::<Vec<_>>();
            let runs = overlay.tab(&lane.tab_id).map_or(&[][..], |tag| tag.runs.as_slice());
            let grouped_ids = grouped_lanes.iter().map(|child| child.tab_id.as_str())
                .collect::<HashSet<_>>();
            let grouped_workflows = workflows.iter().copied()
                .filter(|workflow| parent_for(workflow).is_some_and(|id| grouped_ids.contains(id)))
                .collect::<Vec<_>>();
            let (run_agents, run_workflows) = split_runs(lane, runs);
            let (grouped_agents, grouped_runs) = grouped_lanes.iter()
                .filter_map(|child| overlay.tab(&child.tab_id).map(|tag| split_runs(child, &tag.runs)))
                .fold((0, 0), |(agents, workflows), (a, w)| (agents + a, workflows + w));
            let agents = grouped_lanes.len() + run_agents + grouped_agents;
            let workflow_count = children.len() + grouped_workflows.len() + run_workflows + grouped_runs;
            let running = agents + workflow_count;
            let attention = all_workflows.iter().filter(|workflow| {
                parent_for(workflow) == Some(lane.tab_id.as_str())
                    || parent_for(workflow).is_some_and(|id| grouped_ids.contains(id))
            }).map(|tab| tab.tab_id.as_str())
                .chain(grouped_lanes.iter().map(|tab| tab.tab_id.as_str()))
                .filter_map(|id| overlay.tab(id))
                .map(|tag| tag.attention)
                .chain(runs.iter().map(|run| run.attention))
                .chain(grouped_lanes.iter().filter_map(|lane| overlay.tab(&lane.tab_id))
                    .flat_map(|tag| tag.runs.iter().map(|run| run.attention)))
                .max_by_key(|attention| attention.rank());
            let focused = snapshot.focused_workspace_id.as_deref() == Some(workspace_id)
                && children.iter().chain(grouped_lanes.iter()).chain(grouped_workflows.iter())
                    .any(|tab| snapshot.focused_tab_id.as_deref() == Some(tab.tab_id.as_str()));
            // Running workflows and runs no longer unfold their lane (Alex,
            // 2026-10-05: "workflows show expanded rather than collapsed within
            // agents by default"); the lane row carries their count instead.
            let expanded = tree.factory_expanded(&lane.tab_id, focused,
                attention == Some(crate::factory_overlay::Attention::Act));
            let mut lane_row = factory_row(
                snapshot, rows, overlay, lane, indent,
                !expanded && running > 0, running > 0,
            );
            if let AgentPanelListEntry::FactoryTab(row) = &mut lane_row {
                if attention == Some(crate::factory_overlay::Attention::Act) {
                    row.attention = crate::factory_overlay::Attention::Act;
                } else if attention == Some(crate::factory_overlay::Attention::Warn)
                    && row.attention == crate::factory_overlay::Attention::None {
                    row.attention = crate::factory_overlay::Attention::Warn;
                }
            }
            summarize_factory_parent(&mut lane_row, agents, workflow_count,
                overlay.tab(&lane.tab_id).is_some_and(|tag| tag.busy), false,
                lane.work_status.is_some());
            if grouped {
                if let AgentPanelListEntry::FactoryTab(row) = &mut lane_row {
                    row.background = lane_mode(lane) == TabMode::Parked;
                }
            }
            out.push(lane_row);
            if expanded {
                // Grouped lanes precede workflow runs, both in snapshot tab order.
                for child in grouped_lanes {
                    let mut row = factory_row(snapshot, rows, overlay, child,
                        indent.saturating_add(1), false, false);
                    if grouped && lane_mode(lane) == TabMode::Parked {
                        if let AgentPanelListEntry::FactoryTab(tab) = &mut row {
                            tab.background = true;
                        }
                    }
                    out.push(row);
                    for workflow in workflows.iter().copied()
                        .filter(|workflow| parent_for(workflow) == Some(child.tab_id.as_str())) {
                        let mut row = factory_row(snapshot, rows, overlay, workflow,
                            indent.saturating_add(1), false, false);
                        if grouped && lane_mode(lane) == TabMode::Parked {
                            if let AgentPanelListEntry::FactoryTab(tab) = &mut row {
                                tab.background = true;
                            }
                        }
                        out.push(row);
                    }
                    if let Some(tag) = overlay.tab(&child.tab_id) {
                        for run in &tag.runs {
                            let mut row = factory_run_row(child, run, indent.saturating_add(1));
                            if grouped && lane_mode(lane) == TabMode::Parked {
                                if let AgentPanelListEntry::FactoryTab(tab) = &mut row {
                                    tab.background = true;
                                }
                            }
                            out.push(row);
                        }
                    }
                }
                for child in children {
                    let mut row = factory_row(snapshot, rows, overlay, child,
                        indent.saturating_add(1), false, false);
                    if grouped && lane_mode(lane) == TabMode::Parked {
                        if let AgentPanelListEntry::FactoryTab(tab) = &mut row {
                            tab.background = true;
                        }
                    }
                    out.push(row);
                }
                for run in runs {
                    let mut row = factory_run_row(lane, run, indent.saturating_add(1));
                    if grouped && lane_mode(lane) == TabMode::Parked {
                        if let AgentPanelListEntry::FactoryTab(tab) = &mut row {
                            tab.background = true;
                        }
                    }
                    out.push(row);
                }
            }
    };
    for lane in orchestrator_lanes {
        push_lane(out, lane, indent, false);
    }
    if sectioned {
        let mut first = true;
        for (section, label) in [
            (TabSection::Reviewing, "READY FOR REVIEW"),
            (TabSection::Scoping, "SCOPING"),
            (TabSection::Implementing, "IMPLEMENTING"),
            (TabSection::Monitoring, "MONITORING"),
        ] {
            let members = lanes.iter().copied().filter(|lane| lane_mode(lane) == TabMode::Active
                && grouped_root(&lane.tab_id).is_none() && lane_section(lane) == section)
                .collect::<Vec<_>>();
            if members.is_empty() && (section != TabSection::Implementing || ordinary.is_empty()) {
                continue;
            }
            let right = if section == TabSection::Reviewing {
                members.len().to_string()
            } else if first {
                first = false;
                "⌘1..9".to_owned()
            } else {
                String::new()
            };
            out.push(AgentPanelListEntry::FactorySection { label, right, indent,
                controls: Some(FactorySectionControls { workspace_id: workspace_id.to_owned(), collapsed: false, focused: false,
                    alert: group_state(&members).0 || (section == TabSection::Implementing && group_state(&ordinary).0),
                    count: members.len() + if section == TabSection::Implementing { ordinary.len() } else { 0 } }),
            });
            for lane in members {
                push_lane(out, lane, indent, false);
            }
            if section == TabSection::Implementing {
                for tab in &ordinary {
                    out.push(factory_row(snapshot, rows, overlay, tab, indent, false, false));
                }
            }
        }
    } else if lanes.iter().any(|lane| lane_mode(lane) == TabMode::Active && grouped_root(&lane.tab_id).is_none())
        || !root_workflows.is_empty() || !ordinary.is_empty() {
        out.push(AgentPanelListEntry::FactorySection {
            label: "LANES",
            right: "⌘1..9".to_owned(),
            indent,
            controls: None,
        });
        for lane in &lanes {
            if lane_mode(lane) == TabMode::Active && grouped_root(&lane.tab_id).is_none() {
                push_lane(out, lane, indent, false);
            }
        }
        for workflow in &root_workflows {
            out.push(factory_row(snapshot, rows, overlay, workflow, indent, false, false));
        }
        for tab in ordinary {
            out.push(factory_row(snapshot, rows, overlay, tab, indent, false, false));
        }
    }
    for (mode, group, expanded) in [
        (TabMode::Auto, FactoryGroupKind::Automations, &tree.factory_auto_expanded),
        (TabMode::Parked, FactoryGroupKind::Parked, &tree.factory_parked_expanded),
        (TabMode::Active, FactoryGroupKind::Closed, &tree.factory_idle_expanded),
    ] {
        let members = lanes.iter().copied().filter(|lane| lane_mode(lane) == mode
            && (group != FactoryGroupKind::Closed || (sectioned
                && grouped_root(&lane.tab_id).is_none() && lane_section(lane) == TabSection::Closed)))
            .collect::<Vec<_>>();
        let roots = if sectioned && group == FactoryGroupKind::Automations {
            root_workflows.as_slice()
        } else {
            &[]
        };
        if members.is_empty() && roots.is_empty() {
            continue;
        }
        let mut state_members = members.clone();
        state_members.extend_from_slice(roots);
        let (alert, working) = group_state(&state_members);
        let working = group == FactoryGroupKind::Automations && working;
        let collapsed = !expanded.contains(workspace_id);
        out.push(AgentPanelListEntry::FactoryBackground {
            kind: group, workspace_id: workspace_id.to_owned(), count: members.len() + roots.len(),
            collapsed, indent, alert, working, shortcut: false,
        });
        if !collapsed {
            for lane in members {
                push_lane(out, lane, indent.saturating_add(1), true);
            }
            for workflow in roots {
                out.push(factory_row(snapshot, rows, overlay, workflow, indent.saturating_add(1), false, false));
            }
        }
    }
    let background = tabs
        .iter()
        .copied()
        .filter(|tab| {
            overlay.tab(&tab.tab_id)
                .is_some_and(|tag| (tag.done
                    && !(tag.kind == TabKind::Lane && tag.mode != TabMode::Active)
                    && (tag.kind != TabKind::Workflow || tag.parent.as_deref().is_some_and(|id| {
                        grouped_root(id).is_some()
                    }))) || tag.kind == TabKind::Advisor)
        })
        .collect::<Vec<_>>();
    if !background.is_empty() {
        let collapsed = !tree.factory_background_expanded.contains(workspace_id);
        out.push(AgentPanelListEntry::FactoryBackground {
            kind: FactoryGroupKind::Background,
            workspace_id: workspace_id.to_owned(),
            count: background.len(),
            collapsed,
            indent,
            alert: sectioned && group_state(&background).0,
            working: false,
            shortcut: false,
        });
        if !collapsed {
            for tab in background {
                out.push(factory_row(
                    snapshot,
                    rows,
                    overlay,
                    tab,
                    indent.saturating_add(1),
                    false,
                    false,
                ));
            }
        }
    }
    if sectioned && super::sidebar_report::recording() {
        let mut report = super::sidebar_report::WorkspaceReport::new(workspace_id, tree, filter);
        report.placements(&out[start..]);
        report.kinds(overlay);
        for tab in snapshot.tabs.iter().filter(|tab| tab.workspace_id == workspace_id) {
            let Some(tag) = overlay.tab(&tab.tab_id) else { continue };
            if report.tabs.iter().any(|entry| entry.tab == tab.tab_id) { continue; }
            let under = if tag.kind == TabKind::Workflow {
                parent_for(tab).map(str::to_owned)
            } else { grouped_root(&tab.tab_id) };
            let parent = under.as_deref().and_then(|id| report.tabs.iter().find(|entry| entry.tab == id));
            let section = parent.and_then(|entry| entry.section.clone()).or_else(|| {
                let label = if tag.done || tag.kind == TabKind::Advisor { Some("background") }
                    else if tag.mode == TabMode::Parked { Some("parked") }
                    else if tag.mode == TabMode::Auto || (tag.kind == TabKind::Workflow && under.is_none()) { Some("services") }
                    else { super::sidebar_report::section_label(tag.section) };
                label.map(str::to_owned)
            });
            let hidden = if !tabs.iter().any(|candidate| candidate.tab_id == tab.tab_id) { "goal_filter" }
                else if tag.done || tag.kind == TabKind::Advisor { "background" }
                else if tag.mode == TabMode::Parked { "parked_folded" }
                else if tag.mode == TabMode::Auto { "auto_folded" }
                else if tag.section == Some(TabSection::Closed) { "closed_folded" }
                else if under.is_some() { "group_folded" }
                else if tag.kind == TabKind::Workflow && first_orchestrator.is_none() { "services_folded" }
                else { "unplaced" };
            report.add(&tab.tab_id, tag.kind, section, under, hidden);
        }
        report.inherit_sections();
        apply_factory_sections(out, start, tree, workspace_id, indent);
        report.visibility(&out[start..], tree);
        super::sidebar_report::record(report);
    } else if sectioned {
        apply_factory_sections(out, start, tree, workspace_id, indent);
    }
}

/// Apply client-only visibility after grouping, preserving all existing row semantics.
fn apply_factory_sections(out: &mut Vec<AgentPanelListEntry>, start: usize, tree: &ClientTreeChrome, workspace_id: &str, indent: u8) {
    let entries = out.drain(start..).collect::<Vec<_>>();
    let focus = tree.factory_section_focus.get(workspace_id);
    let mut entries = entries.into_iter().peekable();
    let mut hidden = 0;
    let mut hidden_alert = false;
    let mut shortcut = true;
    while let Some(mut entry) = entries.next() {
        let mut children = Vec::new();
        while entries.peek().is_some_and(|entry| !matches!(entry,
            AgentPanelListEntry::FactorySection { .. } | AgentPanelListEntry::FactoryBackground { .. })) {
            if let Some(child) = entries.next() { children.push(child); }
        }
        let row_alert = |entry: &AgentPanelListEntry| match entry {
            AgentPanelListEntry::FactoryTab(row) => row.status == crate::api::schema::AgentStatus::Blocked
                || row.attention == crate::factory_overlay::Attention::Act
                || row.header.child_states.contains(&crate::api::schema::AgentStatus::Blocked),
            AgentPanelListEntry::FactoryBackground { alert, .. } => *alert,
            _ => false,
        };
        let alert = row_alert(&entry) || children.iter().any(row_alert)
            || matches!(&entry, AgentPanelListEntry::FactorySection { controls: Some(controls), .. } if controls.alert);
        let count = match &entry {
            AgentPanelListEntry::FactoryBackground { count, .. } => *count,
            AgentPanelListEntry::FactorySection { controls: Some(controls), .. } => controls.count,
            _ => children.len(),
        };
        let visible = match &entry {
            AgentPanelListEntry::FactorySection { label, .. } => focus.is_none_or(|focused| *label == "ORCHESTRATOR" || focused == label),
            _ => focus.is_none(),
        };
        if !visible {
            hidden += count.max(children.len());
            hidden_alert |= alert;
            continue;
        }
        let mut collapsed = false;
        if let AgentPanelListEntry::FactorySection { label, right, controls, .. } = &mut entry {
            collapsed = focus.is_none() && tree.factory_sections_collapsed.contains(&format!("{workspace_id}:{label}"));
            if focus.is_some() && *label != "ORCHESTRATOR" && *label != "READY FOR REVIEW" && shortcut {
                *right = "⌘1..9".to_owned();
                shortcut = false;
            }
            if collapsed {
                let hint = if right.contains("⌘") { format!(" {right}") } else { String::new() };
                *right = format!("{count}{}{hint}", if alert { "!" } else { "" });
            }
            *controls = Some(FactorySectionControls { workspace_id: workspace_id.to_owned(), collapsed,
                focused: focus.is_some_and(|focused| focused == label), alert: collapsed && alert, count });
        }
        out.push(entry);
        if !collapsed { out.extend(children); }
    }
    if focus.is_some() {
        out.push(AgentPanelListEntry::FactoryShowAll { workspace_id: workspace_id.to_owned(), count: hidden, alert: hidden_alert, indent });
    }
}

fn factory_run_row(
    parent: &crate::protocol::ClientShellTab,
    run: &crate::factory_overlay::RunTag,
    indent: u8,
) -> AgentPanelListEntry {
    let done = run_done(parent, run);
    AgentPanelListEntry::FactoryTab(FactoryTabRow {
        header: TreeHeader {
            workspace_id: parent.workspace_id.clone(),
            tab_id: Some(parent.tab_id.clone()),
            label: run.name.clone().unwrap_or_else(|| run.id.clone()),
            key: factory_run_key(&parent.tab_id, &run.id),
            collapsed: false,
            child_states: Vec::new(),
            collapsible: false,
            pinned: false,
            indent,
            active: false,
            group: None,
            space_attention: None,
            factory_space: false,
        },
        status: if done { crate::api::schema::AgentStatus::Done } else { crate::api::schema::AgentStatus::Working },
        reviewing: false,
        review_url: None,
        scoping: false,
        scope_url: None,
        badge: run.badge.clone(),
        machine: None,
        phase: run.phase.clone(),
        started: run.started,
        summary: None,
        attention: run.attention,
        idle: false,
        idle_reason: None,
        devloop: false,
        background: false,
        workflow: true,
        done,
    })
}

/// Collapse key of a workflow run row: the run lives inside its lane's tab, so
/// the row carries the lane's tab id and is told apart by this key alone.
fn factory_run_key(tab_id: &str, run_id: &str) -> String {
    format!("{tab_id}{FACTORY_RUN_KEY_MARK}{run_id}")
}

const FACTORY_RUN_KEY_MARK: &str = "#run:";

/// Whether a tree header with this tab id and key is a workflow run row.
pub(super) fn is_factory_run_key(tab_id: &str, key: &str) -> bool {
    key.strip_prefix(tab_id)
        .is_some_and(|rest| rest.starts_with(FACTORY_RUN_KEY_MARK))
}

/// Whether a run under `parent` is over. The agent-rails overlay names the
/// chat's own Claude subagents and teammates agent:<id>; once the endpoint
/// reports the chat quiet, its screen has said they are over, even while the
/// overlay still lists them (server `app/work_status.rs`).
fn run_done(parent: &crate::protocol::ClientShellTab, run: &crate::factory_overlay::RunTag) -> bool {
    run.done
        || run.id.starts_with("agent:")
            && parent.work_status.is_some_and(|status| !matches!(status,
                crate::api::schema::AgentStatus::Working | crate::api::schema::AgentStatus::Blocked))
}

fn split_runs(parent: &crate::protocol::ClientShellTab, runs: &[crate::factory_overlay::RunTag]) -> (usize, usize) {
    // The agent-rails overlay names Agent-tool subagents agent:<id> and workflow runs wf_<id>.
    runs.iter().filter(|run| !run_done(parent, run)).fold((0, 0), |(agents, workflows), run| {
        if run.id.starts_with("agent:") { (agents + 1, workflows) } else { (agents, workflows + 1) }
    })
}

/// Count a parent's live children into its summary. `reported` is a parent
/// whose endpoint already said whether the chat works: grouped lanes are
/// their own chats and only its own live runs count, which the endpoint
/// already folded in, so the counts never repaint its state.
fn summarize_factory_parent(row: &mut AgentPanelListEntry, agents: usize, workflows: usize, busy: bool, orchestrator: bool, reported: bool) {
    if let AgentPanelListEntry::FactoryTab(row) = row {
        if !reported && ((agents + workflows > 0 && matches!(row.status, crate::api::schema::AgentStatus::Idle
                | crate::api::schema::AgentStatus::Done
                | crate::api::schema::AgentStatus::Unknown))
            || (busy && matches!(row.status, crate::api::schema::AgentStatus::Idle
                | crate::api::schema::AgentStatus::Done)))
        {
            row.status = crate::api::schema::AgentStatus::Working;
            row.idle = false;
        }
        let tag_summary = orchestrator.then(|| row.summary.as_deref().unwrap_or("").trim())
            .filter(|summary| !summary.is_empty());
        row.summary = Some(if agents + workflows > 0 {
            let mut parts = Vec::new();
            if agents > 0 {
                parts.push(format!("{agents} {}", if agents == 1 { "agent" } else { "agents" }));
            }
            if workflows > 0 {
                parts.push(format!("{workflows} {}", if workflows == 1 { "workflow" } else { "workflows" }));
            }
            if let Some(summary) = tag_summary {
                parts.push(summary.to_owned());
            }
            parts.join(" · ")
        } else if let Some(summary) = tag_summary {
            summary.to_owned()
        } else if row.idle && row.idle_reason.is_none() {
            "idle".to_owned()
        } else {
            String::new()
        });
    }
}

fn factory_row(
    snapshot: &ClientShellSnapshot,
    rows: &[AgentRow],
    overlay: &crate::factory_overlay::FactoryOverlay,
    tab: &crate::protocol::ClientShellTab,
    indent: u8,
    collapsed: bool,
    collapsible: bool,
) -> AgentPanelListEntry {
    let tag = overlay.tab(&tab.tab_id);
    let status = chat_status(
        tab.work_status,
        rows.iter().filter(|row| row.tab_id == tab.tab_id).map(|row| row.status),
        tab.agent_status,
        tag,
    );
    let background =
        tag.is_some_and(|tag| tag.done || tag.kind == crate::factory_overlay::TabKind::Advisor);
    AgentPanelListEntry::FactoryTab(FactoryTabRow {
        header: TreeHeader {
            workspace_id: tab.workspace_id.clone(),
            tab_id: Some(tab.tab_id.clone()),
            label: {
                let name = tag.and_then(|tag| tag.name.clone())
                    .unwrap_or_else(|| tab.label.clone());
                if tag.is_some_and(|tag| tag.kind == crate::factory_overlay::TabKind::Workflow) {
                    name.strip_prefix("wf ").unwrap_or(&name).to_owned()
                } else if tag.is_some_and(|tag| tag.kind == crate::factory_overlay::TabKind::Lane) {
                    let name = if name.get(..10).is_some_and(|prefix| prefix.eq_ignore_ascii_case("[scoping] ")) {
                        &name[10..]
                    } else {
                        &name
                    };
                    let name = match name.rsplit_once(" · ") {
                        Some((label, stage)) if ["scoping", "implementing", "reviewing", "monitoring", "closed"]
                            .iter().any(|candidate| stage.eq_ignore_ascii_case(candidate)) => label,
                        _ => name,
                    };
                    name.to_owned()
                } else {
                    name
                }
            },
            key: tab.tab_id.clone(),
            collapsed,
            child_states: Vec::new(),
            collapsible,
            pinned: snapshot
                .pinned_tabs
                .iter()
                .any(|pin| pin.tab_id == tab.tab_id),
            indent,
            active: snapshot.focused_workspace_id.as_deref() == Some(tab.workspace_id.as_str())
                && tab.focused,
            group: None,
            space_attention: None,
            factory_space: false,
        },
        status,
        reviewing: !background && tag.is_some_and(|tag| tag.kind == crate::factory_overlay::TabKind::Lane
            && tag.section == Some(crate::factory_overlay::TabSection::Reviewing)),
        review_url: tag.and_then(|tag| tag.review_url.clone()),
        scoping: !background && tag.is_some_and(|tag| tag.kind == crate::factory_overlay::TabKind::Lane
            && tag.section == Some(crate::factory_overlay::TabSection::Scoping)),
        scope_url: tag.and_then(|tag| tag.scope_url.clone()),
        badge: tag.and_then(|tag| tag.badge.clone()),
        machine: snapshot
            .panes
            .iter()
            .filter(|pane| pane.tab_id == tab.tab_id)
            .find_map(|pane| pane.machine.clone()),
        phase: tag.and_then(|tag| tag.phase.clone()),
        started: tag.and_then(|tag| tag.started),
        summary: tag.and_then(|tag| tag.summary.clone()),
        attention: tag.map_or(crate::factory_overlay::Attention::None, |tag| tag.attention),
        idle: lane_is_idle(tag, status, tab.work_status.is_some()),
        idle_reason: tag.and_then(|tag| tag.idle_reason.clone()),
        devloop: tag.is_some_and(|tag| tag.kind == crate::factory_overlay::TabKind::Lane && tag.devloop),
        background,
        workflow: tag.is_some_and(|tag| tag.kind == crate::factory_overlay::TabKind::Workflow),
        done: tag.is_some_and(|tag| tag.done),
    })
}

/// One chat's state. The endpoint's `work_status` is the one rule every
/// surface draws (server `app/work_status.rs`); only an endpoint that predates
/// it falls back to the agents: a finished pane must not mask another pane
/// still working, and a busy tag reads as working. Shared by the spaces tree's
/// factory rows and the pinned section so both draw the same glyph. Leave
/// status_priority (used by non-factory rows) unchanged.
fn chat_status(
    reported: Option<crate::api::schema::AgentStatus>,
    agents: impl Iterator<Item = crate::api::schema::AgentStatus>,
    fallback: crate::api::schema::AgentStatus,
    tag: Option<&crate::factory_overlay::TabTag>,
) -> crate::api::schema::AgentStatus {
    if let Some(status) = reported {
        return status;
    }
    let status = agents
        .max_by_key(|status| match status {
            crate::api::schema::AgentStatus::Blocked => 4,
            crate::api::schema::AgentStatus::Working => 3,
            crate::api::schema::AgentStatus::Done => 2,
            crate::api::schema::AgentStatus::Idle => 1,
            crate::api::schema::AgentStatus::Unknown => 0,
        })
        .unwrap_or(fallback);
    if tag.is_some_and(|tag| tag.busy)
        && matches!(status, crate::api::schema::AgentStatus::Idle | crate::api::schema::AgentStatus::Done)
    {
        crate::api::schema::AgentStatus::Working
    } else {
        status
    }
}

/// A factory lane with no work and nothing to say: drawn with the idle ring.
/// A `reported` status already weighed the lane's work, so the overlay's
/// busy copy does not hold the ring off.
fn lane_is_idle(
    tag: Option<&crate::factory_overlay::TabTag>,
    status: crate::api::schema::AgentStatus,
    reported: bool,
) -> bool {
    tag.is_some_and(|tag| tag.kind == crate::factory_overlay::TabKind::Lane
        && (reported || !tag.busy)
        && tag.summary.as_deref().is_none_or(|summary| summary.trim().is_empty()))
        && status == crate::api::schema::AgentStatus::Idle
}

/// Gather whole space blocks under the overlay's named space groups (Rails,
/// Open Factory, ...), in group order, keeping the manual order inside each
/// group. Spaces no group names stay after the groups, without a header.
fn group_spaces(
    entries: Vec<AgentPanelListEntry>,
    overlay: Option<&crate::factory_overlay::FactoryOverlay>,
) -> Vec<AgentPanelListEntry> {
    let Some(overlay) = overlay.filter(|overlay| !overlay.space_groups.is_empty()) else {
        return entries;
    };
    let mut groups: Vec<Vec<AgentPanelListEntry>> =
        overlay.space_groups.iter().map(|_| Vec::new()).collect();
    let mut prefix = Vec::new();
    let mut rest = Vec::new();
    let mut current: Option<usize> = None;
    for entry in entries {
        if matches!(
            entry,
            AgentPanelListEntry::FactoryGoalPicker { .. }
                | AgentPanelListEntry::PinnedChatsHeader
                | AgentPanelListEntry::PinnedTab(_)
        ) {
            // As in reorder_spaces: the goal picker and the pinned section sit
            // above every space, so no group header may come before them.
            prefix.push(entry);
            continue;
        }
        if let AgentPanelListEntry::SpaceHeader(header) = &entry {
            current = overlay.space_group(&header.workspace_id, &header.label);
        }
        match current {
            Some(index) => groups[index].push(entry),
            None => rest.push(entry),
        }
    }
    let mut out = prefix;
    for (group, members) in overlay.space_groups.iter().zip(groups) {
        if members.is_empty() {
            continue;
        }
        out.push(AgentPanelListEntry::SpaceGroupHeader { name: group.name.clone() });
        out.extend(members);
    }
    out.extend(rest);
    out
}

/// Reorder whole space blocks to follow the manual drag order. Blocks not named
/// in `space_order` keep their relative position behind the ones that are.
fn reorder_spaces(
    entries: Vec<AgentPanelListEntry>,
    space_order: &[String],
) -> Vec<AgentPanelListEntry> {
    if space_order.is_empty() {
        return entries;
    }
    let mut blocks = Vec::<(Option<String>, Vec<AgentPanelListEntry>)>::new();
    let mut prefix = Vec::new();
    for entry in entries {
        if matches!(
            entry,
            AgentPanelListEntry::FactoryGoalPicker { .. }
                | AgentPanelListEntry::PinnedChatsHeader
                | AgentPanelListEntry::PinnedTab(_)
        ) {
            // The goal picker and the pinned section sit above the spaces and
            // are never reordered with them.
            prefix.push(entry);
            continue;
        }
        match &entry {
            AgentPanelListEntry::SpaceHeader(header) => {
                blocks.push((Some(header.workspace_id.clone()), vec![entry]));
            }
            _ => match blocks.last_mut() {
                Some((_, block)) => block.push(entry),
                None => blocks.push((None, vec![entry])),
            },
        }
    }
    let rank = |workspace_id: &Option<String>| {
        workspace_id
            .as_ref()
            .and_then(|id| space_order.iter().position(|saved| saved == id))
            .unwrap_or(usize::MAX)
    };
    let mut ordered = blocks.into_iter().enumerate().collect::<Vec<_>>();
    ordered.sort_by_key(|(index, (workspace_id, _))| (rank(workspace_id), *index));
    prefix.into_iter().chain(ordered.into_iter().flat_map(|(_, (_, block))| block)).collect()
}

fn workspace_label(snapshot: &ClientShellSnapshot, workspace_id: &str) -> String {
    snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == workspace_id)
        .map(|workspace| workspace.label.clone())
        .unwrap_or_else(|| workspace_id.to_owned())
}

/// Presentation-only hierarchy data for one agent panel row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct AgentGroupRender {
    /// Nesting depth under the group owner.
    pub(super) depth: u8,
    /// `Some(expanded)` when this agent owns children that a collapse hides.
    pub(super) expanded: Option<bool>,
    /// Descendants hidden by a collapsed group.
    pub(super) hidden_children: usize,
    /// This row is the last child within its parent group.
    pub(super) last_in_group: bool,
    /// Always-visible open-tab count for an orchestrator-mode group owner.
    pub(super) group_count: Option<usize>,
    /// Collapse key for this owner row: its own pane id, or
    /// `orch:<workspace-id>` for an orchestrator group.
    pub(super) group_key: Option<String>,
    /// The server has this group folded (`agent.group.collapse`). A server
    /// fold is shared by every client and the CLI, so the chevron clears it
    /// through the endpoint rather than the local set.
    pub(super) server_collapsed: bool,
    /// Most demanding status among the descendants a fold hides; colors the
    /// `+N` summary so blocked or unread work still shows through.
    pub(super) hidden_status: Option<crate::api::schema::AgentStatus>,
}

/// Reorder agent rows so each agent's children sit directly beneath it,
/// depth-first, dropping descendants of collapsed groups and recording the
/// hidden-descendant count on the collapsed owner row. Roots keep their
/// incoming order; siblings keep their relative order. Cycle-safe: any row
/// unreachable from a root is appended at the end as a root.
/// With tabs shown, orchestrator adoption is disabled: each lane keeps its tab.
pub(super) fn arrange_agent_hierarchy_with(
    snapshot: &ClientShellSnapshot,
    tree: &ClientTreeChrome,
    rows: Vec<AgentRow>,
    adopt: bool,
) -> Vec<AgentRow> {
    let orchestrator_count = |row: &AgentRow| -> Option<usize> {
        // Only the first tab's agent leads an orchestrator group.
        let workspace = snapshot
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == row.workspace_id)
            .filter(|workspace| workspace.orchestrator_mode)?;
        let first_tab = snapshot
            .tabs
            .iter()
            .find(|tab| tab.workspace_id == row.workspace_id)?;
        (first_tab.tab_id == row.tab_id).then(|| workspace.tab_count.saturating_sub(1))
    };
    if rows.len() < 2 {
        let mut rows = rows;
        if let Some(row) = rows.first_mut() {
            row.group.group_count = orchestrator_count(row);
        }
        return rows;
    }

    let index_by_pane = rows
        .iter()
        .enumerate()
        .map(|(index, row)| (row.pane_id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut children = vec![Vec::<usize>::new(); rows.len()];
    let mut has_parent = vec![false; rows.len()];
    // Parent selection, in priority order: a hands-on pin makes the row a root
    // no matter what; an explicit `under` parent that resolves wins over
    // ownership; otherwise the current owner. An explicit parent that does not
    // resolve was flagged orphaned by the server and falls through to the
    // owner edge so the row never disappears.
    for (index, row) in rows.iter().enumerate() {
        if row.placement.hands_on {
            continue;
        }
        let lookup = |pane_id: &String| index_by_pane.get(pane_id).copied();
        let Some(parent) = row
            .placement
            .parent_pane_id
            .as_ref()
            .and_then(lookup)
            .or_else(|| row.owner_pane_id.as_ref().and_then(lookup))
            .filter(|parent| *parent != index)
        else {
            continue;
        };
        children[parent].push(index);
        has_parent[index] = true;
    }

    // Orchestrator-mode workspaces: the first tab's agent adopts the
    // workspace's other top-level agents, so the whole workspace herds as one
    // collapsible group. Ownership and explicit edges win, so a parented agent
    // stays under its parent, and a hands-on agent is never adopted.
    let mut orchestrator_by_workspace = HashMap::<&str, usize>::new();
    let mut group_counts = vec![None; rows.len()];
    for (index, row) in rows.iter().enumerate() {
        if let Some(count) = orchestrator_count(row) {
            orchestrator_by_workspace
                .entry(row.workspace_id.as_str())
                .or_insert(index);
            group_counts[index] = Some(count);
        }
    }
    for index in 0..rows.len() {
        if !adopt || has_parent[index] || rows[index].placement.hands_on {
            continue;
        }
        let Some(&owner) = orchestrator_by_workspace.get(rows[index].workspace_id.as_str()) else {
            continue;
        };
        if owner == index {
            continue;
        }
        children[owner].push(index);
        has_parent[index] = true;
    }

    let mut pending = rows.into_iter().map(Some).collect::<Vec<_>>();
    let mut visited = vec![false; pending.len()];
    let mut arranged = Vec::with_capacity(pending.len());
    for index in 0..pending.len() {
        if visited[index] || has_parent[index] {
            continue;
        }
        push_subtree(
            index,
            0,
            false,
            !adopt,
            tree,
            &mut pending,
            &children,
            &group_counts,
            &mut visited,
            &mut arranged,
        );
    }
    // Defensive: anything unreachable from a root (a stale or cyclic owner
    // edge) is still listed, as a root.
    for index in 0..pending.len() {
        push_subtree(
            index,
            0,
            false,
            !adopt,
            tree,
            &mut pending,
            &children,
            &group_counts,
            &mut visited,
            &mut arranged,
        );
    }
    arranged
}

/// Count the descendants a fold hides and pick the most demanding status
/// among them, marking each one visited so it is not listed elsewhere.
fn hidden_descendants(
    index: usize,
    pending: &[Option<AgentRow>],
    children: &[Vec<usize>],
    visited: &mut [bool],
    status: &mut Option<crate::api::schema::AgentStatus>,
) -> usize {
    let mut count = 0;
    for &child in &children[index] {
        if visited[child] {
            continue;
        }
        visited[child] = true;
        count += 1;
        if let Some(row) = pending[child].as_ref() {
            if status.is_none_or(|current| {
                cycle_attention_rank(row.status) > cycle_attention_rank(current)
            }) {
                *status = Some(row.status);
            }
        }
        count += hidden_descendants(child, pending, children, visited, status);
    }
    count
}

#[allow(clippy::too_many_arguments)] // mirrors the fork's captured recursion context
fn push_subtree(
    index: usize,
    depth: u8,
    last_in_group: bool,
    default_collapsed: bool,
    tree: &ClientTreeChrome,
    pending: &mut [Option<AgentRow>],
    children: &[Vec<usize>],
    group_counts: &[Option<usize>],
    visited: &mut [bool],
    arranged: &mut Vec<AgentRow>,
) {
    if visited[index] {
        return;
    }
    visited[index] = true;
    let Some(mut row) = pending[index].take() else {
        return;
    };
    row.group.depth = depth;
    row.group.last_in_group = last_in_group;
    row.group.group_count = group_counts[index];
    let child_indexes = children[index]
        .iter()
        .copied()
        .filter(|child| !visited[*child])
        .collect::<Vec<_>>();
    if child_indexes.is_empty() {
        arranged.push(row);
        return;
    }
    let group_key = if group_counts[index].is_some() {
        Some(format!("orch:{}", row.workspace_id))
    } else {
        Some(row.pane_id.clone())
    };
    // Folded when this client folded it locally or the server holds the fold
    // (`herdr agent group collapse`, or another client's chevron).
    // With groups folded by default, the set lists the groups opened by hand.
    let listed = group_key
        .as_ref()
        .is_some_and(|key| tree.collapsed_agent_groups.contains(key));
    let expanded = !row.placement.collapsed && (listed == default_collapsed);
    row.group.expanded = Some(expanded);
    row.group.group_key = group_key;
    row.group.server_collapsed = row.placement.collapsed;
    if !expanded {
        let mut status = None;
        row.group.hidden_children =
            hidden_descendants(index, pending, children, visited, &mut status);
        row.group.hidden_status = status;
        arranged.push(row);
        return;
    }
    arranged.push(row);
    let last = child_indexes.len().saturating_sub(1);
    for (position, child) in child_indexes.into_iter().enumerate() {
        push_subtree(
            child,
            depth.saturating_add(1),
            position == last,
            default_collapsed,
            tree,
            pending,
            children,
            group_counts,
            visited,
            arranged,
        );
    }
}

/// The agent groups that hold `pane_id` in the tree, nearest first, walked by
/// the same parent rule as [`arrange_agent_hierarchy_with`]: a hands-on pin stops
/// the walk, an explicit parent wins over the owner. The last entry is the
/// orchestrator row when the walk ends on an unpinned root of an
/// orchestrator-mode workspace. Each entry is (local collapse key, owner).
pub(super) fn agent_group_ancestors<'a>(
    snapshot: &'a ClientShellSnapshot,
    pane_id: &str,
) -> Vec<(String, &'a crate::protocol::ClientShellAgent)> {
    let find = |pane_id: &str| {
        snapshot
            .agents
            .iter()
            .find(|agent| agent.pane_id == pane_id)
    };
    let mut ancestors = Vec::new();
    let mut visited = HashSet::new();
    let Some(mut current) = find(pane_id) else {
        return ancestors;
    };
    loop {
        if !visited.insert(current.pane_id.clone()) || current.group.hands_on {
            return ancestors;
        }
        let parent = current
            .group
            .parent_pane_id
            .as_deref()
            .and_then(find)
            .or_else(|| current.owner_pane_id.as_deref().and_then(find));
        match parent {
            Some(parent) => {
                ancestors.push((parent.pane_id.clone(), parent));
                current = parent;
            }
            None => break,
        }
    }
    // An unpinned root: an orchestrator workspace's first-tab agent adopts it.
    let orchestrator = snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == current.workspace_id)
        .filter(|workspace| workspace.orchestrator_mode)
        .and_then(|workspace| {
            let first_tab = snapshot
                .tabs
                .iter()
                .find(|tab| tab.workspace_id == workspace.workspace_id)?;
            snapshot
                .agents
                .iter()
                .find(|agent| agent.tab_id == first_tab.tab_id)
        });
    if let Some(orchestrator) = orchestrator.filter(|agent| agent.pane_id != current.pane_id) {
        ancestors.push((format!("orch:{}", current.workspace_id), orchestrator));
    }
    ancestors
}

/// Candidate sidebar parents for the agent in `pane_id`: "Automatic" first,
/// then every other agent in the same space in panel order. A descendant can
/// never be the parent, so it is left out rather than offered as a choice the
/// endpoint would reject.
pub(super) fn nest_under_entries(
    snapshot: &ClientShellSnapshot,
    sort: crate::config::AgentPanelSortConfig,
    pane_id: &str,
) -> Vec<ClientNestUnderEntry> {
    let Some(agent) = snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == pane_id)
    else {
        return Vec::new();
    };
    let current = agent.group.parent_pane_id.as_deref();
    let mark = |label: String, is_current: bool| {
        if is_current {
            format!("{label} \u{2713}")
        } else {
            label
        }
    };
    let mut entries = vec![ClientNestUnderEntry {
        parent_pane_id: None,
        label: mark(
            "Automatic".into(),
            current.is_none() && !agent.group.hands_on,
        ),
    }];
    for candidate_id in super::agent_sidebar::ordered_agent_pane_ids(snapshot, sort) {
        let Some(candidate) = snapshot
            .agents
            .iter()
            .find(|candidate| candidate.pane_id == candidate_id)
        else {
            continue;
        };
        if candidate.pane_id == pane_id || candidate.workspace_id != agent.workspace_id {
            continue;
        }
        let descends = agent_group_ancestors(snapshot, &candidate.pane_id)
            .iter()
            .any(|(_, ancestor)| ancestor.pane_id == pane_id);
        if descends {
            continue;
        }
        let name = candidate
            .display_agent
            .as_deref()
            .or(candidate.name.as_deref())
            .or(candidate.title.as_deref())
            .or(candidate.agent.as_deref())
            .filter(|name| !name.is_empty() && *name != candidate.pane_id);
        let label = match name {
            Some(name) => format!("{name}  {}", candidate.pane_id),
            None => candidate.pane_id.clone(),
        };
        entries.push(ClientNestUnderEntry {
            parent_pane_id: Some(candidate.pane_id.clone()),
            label: mark(label, current == Some(candidate.pane_id.as_str())),
        });
    }
    entries
}

/// One entry in the flat ⌘E rotation, in agent-panel order.
pub(super) struct AgentCycleEntry {
    pub(super) pane_id: String,
    pub(super) workspace_id: String,
    pub(super) tab_id: String,
    pub(super) status: crate::api::schema::AgentStatus,
}

impl ClientShellState {
    /// Fold or open one agent group from its chevron.
    ///
    /// The endpoint holds the fold when it can (`agent.group.collapse`), so
    /// the chevron, `herdr agent group collapse` and every other client agree.
    /// A fold this client made on its own (before the endpoint knew the
    /// method, or against an endpoint that still does not) stays in the local
    /// set, and opening the group clears both.
    /// The tree with tabs shown folds every agent group until it is opened
    /// by hand (Alex, 2026-09-28: lanes' workflows collapsed by default).
    pub(super) fn groups_fold_by_default(&self) -> bool {
        let tree = self
            .tree_chrome
            .get(&self.active_endpoint_id)
            .unwrap_or(&self.tree_chrome_default);
        tree_view_active(&self.config) && tree.show_tabs
    }

    pub(super) fn toggle_agent_group(
        &mut self,
        hit: &AgentGroupHit,
        outcome: &mut ClientShellInput,
    ) {
        let method = |collapsed: bool| {
            crate::api::schema::Method::AgentGroupCollapse(
                crate::api::schema::AgentGroupCollapseParams {
                    target: hit.owner_pane_id.clone(),
                    collapsed,
                },
            )
        };
        if self.groups_fold_by_default() {
            // Folded by default: the local set lists the groups opened by hand.
            let tree = self.tree_chrome_mut();
            if hit.expanded {
                tree.collapsed_agent_groups.remove(&hit.key);
            } else {
                tree.collapsed_agent_groups.insert(hit.key.clone());
            }
            self.persist_chrome_preferences(outcome);
            if !hit.expanded
                && hit.server_collapsed
                && self.supports_endpoint_method(&method(false))
            {
                self.push_endpoint_method(method(false), outcome);
            }
            outcome.repaint = true;
            return;
        }
        let endpoint_folds = self.supports_endpoint_method(&method(true));
        if hit.expanded {
            if endpoint_folds {
                self.push_endpoint_method(method(true), outcome);
            } else {
                let tree = self.tree_chrome_mut();
                tree.collapsed_agent_groups.insert(hit.key.clone());
                self.persist_chrome_preferences(outcome);
            }
        } else {
            let tree = self.tree_chrome_mut();
            if tree.collapsed_agent_groups.remove(&hit.key) {
                self.persist_chrome_preferences(outcome);
            }
            if hit.server_collapsed && endpoint_folds {
                self.push_endpoint_method(method(false), outcome);
            }
        }
        outcome.repaint = true;
    }

    /// The group the `toggle_agent_group` key acts on for `pane_id`: the group
    /// this agent owns when it owns one, otherwise the nearest group holding
    /// it. Derived from the snapshot, so a row hidden inside a fold can still
    /// reopen it from the keyboard.
    pub(super) fn agent_group_for_pane(&self, pane_id: &str) -> Option<AgentGroupHit> {
        let snapshot = self.snapshot.as_deref()?;
        let tree = self
            .tree_chrome
            .get(&self.active_endpoint_id)
            .unwrap_or(&self.tree_chrome_default);
        let own = snapshot.agents.iter().find_map(|agent| {
            agent_group_ancestors(snapshot, &agent.pane_id)
                .into_iter()
                .next()
                .filter(|(_, owner)| owner.pane_id == pane_id)
        });
        let (key, owner) =
            own.or_else(|| agent_group_ancestors(snapshot, pane_id).into_iter().next())?;
        Some(AgentGroupHit {
            rect: Rect::default(),
            expanded: !owner.group.collapsed
                && (tree.collapsed_agent_groups.contains(&key) == self.groups_fold_by_default()),
            key,
            owner_pane_id: owner.pane_id.clone(),
            server_collapsed: owner.group.collapsed,
        })
    }

    /// The agent panel's flat order, minus anything the tree has folded away.
    /// What was folded should not catch ⌘E, so its agents leave the attention
    /// rotation until the space reopens.
    pub(super) fn agent_cycle_candidates(
        &self,
        snapshot: &ClientShellSnapshot,
    ) -> Vec<AgentCycleEntry> {
        let tree = self
            .tree_chrome
            .get(&self.active_endpoint_id)
            .unwrap_or(&self.tree_chrome_default);
        let skips_space = |workspace_id: &str| {
            tree_view_active(&self.config)
                && tree.show_spaces
                && tree.collapsed_spaces.contains(workspace_id)
        };
        // With tab headers shown, a nested workflow's tab is not a cycling
        // destination, even when the group is expanded or needs attention.
        // Use the same arranged hierarchy as the panel so folded children and
        // depth > 0 rows are both excluded without changing the flat order.
        let top_level = (tree_view_active(&self.config)
            && tree.show_tabs
            && snapshot.agent_view_label.is_none())
        .then(|| {
            let rows = super::agent_sidebar::agent_rows(snapshot, &self.config, None);
            let (rows, _) = partition_automations(snapshot, &self.config, rows);
            arrange_agent_hierarchy_with(snapshot, tree, rows, false)
                .into_iter()
                .filter(|row| row.group.depth == 0)
                .map(|row| row.pane_id)
                .collect::<HashSet<_>>()
        });
        super::agent_sidebar::ordered_agent_pane_ids(snapshot, self.config.agent_panel_sort)
            .into_iter()
            .filter_map(|pane_id| {
                if top_level
                    .as_ref()
                    .is_some_and(|visible| !visible.contains(&pane_id))
                {
                    return None;
                }
                let agent = snapshot
                    .agents
                    .iter()
                    .find(|agent| agent.pane_id == pane_id)?;
                (!skips_space(&agent.workspace_id)).then(|| AgentCycleEntry {
                    pane_id,
                    workspace_id: agent.workspace_id.clone(),
                    tab_id: agent.tab_id.clone(),
                    status: agent.agent_status,
                })
            })
            .collect()
    }

    /// ⌘E target selection over the flat panel entries. Returns the index to
    /// focus.
    pub(super) fn agent_cycle_target(
        &self,
        snapshot: &ClientShellSnapshot,
        entries: &[AgentCycleEntry],
        forward: bool,
    ) -> Option<usize> {
        if entries.is_empty() {
            return None;
        }
        let tree = self
            .tree_chrome
            .get(&self.active_endpoint_id)
            .unwrap_or(&self.tree_chrome_default);
        let focused = snapshot.focused_pane_id.as_deref();
        let current_idx =
            focused.and_then(|pane_id| entries.iter().position(|entry| entry.pane_id == pane_id));
        // Priority and triage already sort the panel by attention, so their
        // positional order IS the attention order. Spaces and tree hold a
        // deliberately stable order, so the key must rank for itself there.
        let panel_is_attention_sorted = matches!(
            self.config.agent_panel_sort,
            crate::config::AgentPanelSortConfig::Priority
                | crate::config::AgentPanelSortConfig::Triage
        );
        // Recomputed on every press: the ranking is read fresh from current
        // agent state, so a completion landing between presses is picked up.
        let ranked = |idx: usize| {
            if panel_is_attention_sorted {
                0
            } else {
                cycle_attention_rank(entries[idx].status)
            }
        };
        // With tab headers visible the tab is the unit being navigated, so a
        // press moves to the next TAB rather than the next agent inside the
        // current one. A tab ranks by its most demanding agent, and focus lands
        // on that agent.
        if tree_view_active(&self.config) && tree.show_tabs {
            let current_tab = current_idx
                .map(|idx| (&entries[idx].workspace_id, &entries[idx].tab_id))
                .map(|(workspace_id, tab_id)| (workspace_id.clone(), tab_id.clone()));
            let mut tab_order = Vec::<(String, String)>::new();
            for entry in entries {
                let key = (entry.workspace_id.clone(), entry.tab_id.clone());
                if !tab_order.contains(&key) {
                    tab_order.push(key);
                }
            }
            if tab_order.len() > 1 {
                let here = current_tab
                    .as_ref()
                    .and_then(|key| tab_order.iter().position(|candidate| candidate == key));
                let order = rotation(tab_order.len(), here, forward);
                let tab_rank = |index: usize| {
                    let key = &tab_order[index];
                    entries
                        .iter()
                        .filter(|entry| (&entry.workspace_id, &entry.tab_id) == (&key.0, &key.1))
                        .map(|entry| cycle_attention_rank(entry.status))
                        .max()
                        .unwrap_or(0)
                };
                let best = order
                    .iter()
                    .map(|index| tab_rank(*index))
                    .max()
                    .unwrap_or(0);
                if let Some(target) = order.into_iter().find(|index| tab_rank(*index) == best) {
                    let key = &tab_order[target];
                    // Inside the chosen tab, land on its most demanding agent.
                    if let Some(idx) = entries
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| {
                            (&entry.workspace_id, &entry.tab_id) == (&key.0, &key.1)
                        })
                        .max_by_key(|(_, entry)| cycle_attention_rank(entry.status))
                        .map(|(idx, _)| idx)
                    {
                        return Some(idx);
                    }
                }
            }
        }

        // Candidates exclude whatever is focused now. Ranking the current agent
        // alongside the rest is what made the key look dead: when the focused
        // agent was the only one at the top rank, the search wrapped straight
        // back onto it.
        let order = rotation(entries.len(), current_idx, forward);
        if order.is_empty() {
            return None;
        }
        // Walk from the neighbour outward and take the first candidate at the
        // highest rank present. Walking in that order, rather than from index
        // zero, makes repeated presses visit every peer at a rank before coming
        // back around.
        let best = order.iter().map(|index| ranked(*index)).max().unwrap_or(0);
        let fallback = order[0];
        Some(
            order
                .into_iter()
                .find(|index| ranked(*index) == best)
                .unwrap_or(fallback),
        )
    }
}

impl ClientShellState {
    /// Open any factory fold enclosing a selected tab; normal pane focus
    /// reveals ordinary space and agent-group folds on confirmation.
    pub(super) fn reveal_attention_tab(&mut self, tab_id: &str) {
        let Some(snapshot) = self.snapshot.as_deref() else {
            return;
        };
        let Some(tab) = snapshot.tabs.iter().find(|tab| tab.tab_id == tab_id) else {
            return;
        };
        let workspace_id = tab.workspace_id.clone();
        let parent = self.factory_overlay()
            .and_then(|overlay| overlay.tab(tab_id))
            .and_then(|tag| tag.parent.clone());
        let tree = self.tree_chrome_mut();
        tree.collapsed_spaces.remove(&workspace_id);
        if let Some(parent) = parent {
            tree.factory_collapsed_lanes.remove(&parent);
            tree.factory_expanded_lanes.insert(parent);
        }
    }

    /// Next pane that needs attention on this endpoint. Keep non-candidates in
    /// the order so a focused working pane is still the rotation's anchor.
    pub(super) fn next_attention_target(
        &self,
        snapshot: &ClientShellSnapshot,
    ) -> Option<(Option<String>, String)> {
        use crate::api::schema::AgentStatus;
        use crate::factory_overlay::{Attention, TabKind, TabMode};

        let overlay = self.factory_overlay();
        let mut entries = Vec::<(Option<String>, String, u8)>::new();
        for workspace in &snapshot.workspaces {
            for tab in snapshot.tabs.iter().filter(|tab| tab.workspace_id == workspace.workspace_id) {
                let tag = overlay.and_then(|overlay| overlay.tab(&tab.tab_id));
                let mode = tag.map_or(TabMode::Active, |tag| tag.mode);
                let parent_mode = tag.and_then(|tag| tag.parent.as_deref())
                    .and_then(|id| overlay.and_then(|overlay| overlay.tab(id)))
                    .map_or(TabMode::Active, |parent| parent.mode);
                let auto = mode == TabMode::Auto;
                let background = tag.is_some_and(|tag| tag.done || tag.kind == TabKind::Advisor)
                    || mode == TabMode::Parked || parent_mode != TabMode::Active;
                let ask = !background && !auto && tag.is_some_and(|tag| tag.attention == Attention::Act);
                let busy = tag.is_some_and(|tag| tag.busy);
                let mut has_agent = false;
                for pane in snapshot.panes.iter().filter(|pane| pane.tab_id == tab.tab_id) {
                    let agent = snapshot.agents.iter().find(|agent| agent.pane_id == pane.pane_id);
                    let rank = match agent.map(|agent| agent.agent_status) {
                        Some(AgentStatus::Blocked) => 3,
                        Some(AgentStatus::Done) if !busy => 2,
                        _ => 0,
                    };
                    let first_agent = agent.is_some() && !has_agent;
                    has_agent |= agent.is_some();
                    entries.push((
                        Some(pane.pane_id.clone()),
                        tab.tab_id.clone(),
                        if auto && parent_mode == TabMode::Active && rank == 3 { 3 }
                        else if background || auto { 0 }
                        else { rank.max(u8::from(ask && first_agent)) },
                    ));
                }
                if !has_agent && ask {
                    entries.push((None, tab.tab_id.clone(), 1));
                }
            }
        }
        let focused = snapshot.focused_pane_id.as_deref();
        let current = entries.iter().position(|(pane_id, tab_id, _)| {
            pane_id.as_deref() == focused && (pane_id.is_some() || snapshot.focused_tab_id.as_deref() == Some(tab_id))
        });
        let order: Vec<_> = rotation(entries.len(), current, true)
            .into_iter()
            .filter(|index| {
                let (pane_id, tab_id, _) = &entries[*index];
                pane_id.is_some() || snapshot.focused_tab_id.as_deref() != Some(tab_id)
            })
            .collect();
        let best = order.iter().map(|index| entries[*index].2).max().unwrap_or(0);
        if best == 0 {
            return None;
        }
        let index = order.into_iter().find(|index| entries[*index].2 == best)?;
        let (pane_id, tab_id, _) = &entries[index];
        Some((pane_id.clone(), tab_id.clone()))
    }
}

/// Indices to visit, starting at the neighbour of `current` and skipping
/// `current` itself.
fn rotation(len: usize, current: Option<usize>, forward: bool) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    let step = |index: usize| {
        if forward {
            (index + 1) % len
        } else {
            (index + len - 1) % len
        }
    };
    let start = match current {
        Some(index) => step(index),
        None => {
            if forward {
                0
            } else {
                len - 1
            }
        }
    };
    let mut order = Vec::with_capacity(len);
    let mut index = start;
    for _ in 0..len {
        if Some(index) != current {
            order.push(index);
        }
        index = step(index);
    }
    order
}
