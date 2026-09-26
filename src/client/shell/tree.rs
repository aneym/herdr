//! The unified space → tab → agent sidebar tree.
//!
//! The fork drew this tree server-side from `AppState`; 0.9 renders the whole
//! shell in the client, so the grouping is rebuilt here over
//! [`ClientShellSnapshot`] and the per-client chrome state in
//! [`ClientTreeChrome`]. The shape, the collapse semantics and the hidden
//! section all match `docs/fork/port-0.9/orig/src/ui/sidebar.rs`.

use super::agent_sidebar::AgentRow;
use super::*;

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
            show_hidden_spaces: false,
            hidden_spaces_expanded: false,
            automations_expanded: false,
            collapsed_agent_groups: HashSet::new(),
            space_order: Vec::new(),
        }
    }
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
        }
    }

    pub(super) fn toggle(set: &mut HashSet<String>, key: String) {
        if !set.remove(&key) {
            set.insert(key);
        }
    }
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
    /// agent rows remain beneath it. Always false on tab headers.
    pub(super) pinned: bool,
    pub(super) indent: u8,
    /// This header's workspace or tab holds the focused pane.
    pub(super) active: bool,
    /// When the agents layer is hidden the header stands in for its agent
    /// rows; if one of them owns a group this carries that group's state, so
    /// the header can show the group chevron and `+N` instead of leaving the
    /// group with no control at all.
    pub(super) group: Option<TreeHeaderGroup>,
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
        if self.expanded || self.hidden_children == 0 {
            return 0;
        }
        format!("+{} ", self.hidden_children).len()
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
    /// An agent in a workspace named by `ui.sidebar.automations.workspaces`.
    Automation(AgentRow),
    /// The collapsible section collecting spaces that were folded away. Only
    /// emitted while the hidden reveal is on.
    HiddenSpacesHeader {
        count: usize,
        collapsed: bool,
    },
    SpaceHeader(TreeHeader),
    TabHeader(TreeHeader),
}

impl AgentPanelListEntry {
    pub(super) fn line_count(&self) -> usize {
        match self {
            Self::Agent(row) | Self::Automation(row) => row.rows.len().max(1),
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
    entries.push(AgentPanelListEntry::AutomationsHeader(
        AutomationSummary::of(&automations),
    ));
    if tree.automations_expanded {
        entries.extend(automations.into_iter().map(AgentPanelListEntry::Automation));
    }
}

pub(super) fn tree_view_active(config: &ClientShellConfig) -> bool {
    config.agent_panel_sort == crate::config::AgentPanelSortConfig::Tree
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
pub(super) fn tree_list_entries(
    snapshot: &ClientShellSnapshot,
    tree: &ClientTreeChrome,
    rows: Vec<AgentRow>,
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

    let mut out = Vec::new();
    // Collapsed spaces move out of their slot and collect under one collapsible
    // section at the bottom, so folding a space away actually clears the row it
    // occupied instead of leaving a stub mid-tree.
    let mut hidden_out = Vec::<AgentPanelListEntry>::new();
    let mut hidden_spaces = HashSet::<String>::new();
    for workspace_id in &workspace_order {
        let Some(workspace_rows) = by_workspace.remove(workspace_id) else {
            continue;
        };
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
                collapsible: !workspace_rows.is_empty() && (tree.show_tabs || tree.show_agents),
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
            }));
            if space_collapsed {
                continue;
            }
        }

        let mut tab_order = Vec::<String>::new();
        let mut by_tab = HashMap::<String, Vec<AgentRow>>::new();
        for row in workspace_rows {
            let tab_id = row.tab_id.clone();
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
            if tree.show_tabs {
                let tab = snapshot.tabs.iter().find(|tab| &tab.tab_id == tab_id);
                let key = tab
                    .map(|tab| tab_key(workspace_id, tab.number))
                    .unwrap_or_else(|| tab_key(workspace_id, 0));
                let collapsed = tree.collapsed_tabs.contains(&key);
                let show_dots = collapsed || !tree.show_agents;
                out.push(AgentPanelListEntry::TabHeader(TreeHeader {
                    workspace_id: workspace_id.clone(),
                    tab_id: Some(tab_id.clone()),
                    label: tab
                        .map(|tab| tab.label.clone())
                        .unwrap_or_else(|| tab_id.clone()),
                    key,
                    collapsed,
                    child_states: if show_dots {
                        rollup_state(&tab_rows)
                    } else {
                        Vec::new()
                    },
                    collapsible: !tab_rows.is_empty() && tree.show_agents,
                    pinned: false,
                    indent: space_indent,
                    active: !tree.show_agents
                        && tab.is_some_and(|tab| tab.focused)
                        && snapshot.focused_workspace_id.as_deref() == Some(workspace_id.as_str()),
                    // A tab whose agent rows are hidden stands in for them, even
                    // when the tab itself was collapsed earlier: that collapse
                    // hides nothing now, and a stale key must not swallow the
                    // only control the group has.
                    group: (!tree.show_agents)
                        .then(|| tree_header_group(&tab_rows))
                        .flatten(),
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
            if listed.contains(workspace_id) || !tree.pinned_spaces.contains(workspace_id) {
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
    let mut out = reorder_spaces(out, &tree.space_order);

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
    for entry in entries {
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
    ordered
        .into_iter()
        .flat_map(|(_, (_, block))| block)
        .collect()
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
pub(super) fn arrange_agent_hierarchy(
    snapshot: &ClientShellSnapshot,
    tree: &ClientTreeChrome,
    rows: Vec<AgentRow>,
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
        if has_parent[index] || rows[index].placement.hands_on {
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
    let expanded = !row.placement.collapsed
        && group_key
            .as_ref()
            .is_none_or(|key| !tree.collapsed_agent_groups.contains(key));
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
/// the same parent rule as [`arrange_agent_hierarchy`]: a hands-on pin stops
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
            expanded: !owner.group.collapsed && !tree.collapsed_agent_groups.contains(&key),
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
        super::agent_sidebar::ordered_agent_pane_ids(snapshot, self.config.agent_panel_sort)
            .into_iter()
            .filter_map(|pane_id| {
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
