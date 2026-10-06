//! Saved display ranks. Session vectors and their positional identities never move.
use crate::config::SidebarPriorityConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rank {
    pub value: u32,
    pub parked: bool,
}

fn glob(pattern: &str, value: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let value = value.to_lowercase();
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == value;
    }
    let Some(mut rest) = value.strip_prefix(parts[0]) else {
        return false;
    };
    for part in &parts[1..parts.len() - 1] {
        let Some(index) = rest.find(part) else {
            return false;
        };
        rest = &rest[index + part.len()..];
    }
    rest.ends_with(parts[parts.len() - 1])
}

fn matching_rank(config: &SidebarPriorityConfig, matches: impl Fn(&str) -> bool) -> Option<Rank> {
    if let Some(index) = config.order.iter().position(|rule| matches(rule)) {
        return Some(Rank {
            value: index as u32,
            parked: false,
        });
    }
    config
        .last
        .iter()
        .position(|rule| matches(rule))
        .map(|index| Rank {
            value: (config.order.len() + 1 + index) as u32,
            parked: true,
        })
}

pub(crate) fn workspace_rank(config: &SidebarPriorityConfig, id: &str, label: &str) -> Rank {
    matching_rank(config, |rule| {
        !rule.starts_with("tab:") && (rule == id || rule.to_lowercase() == label.to_lowercase())
    })
    .unwrap_or(Rank {
        value: config.order.len() as u32,
        parked: false,
    })
}

pub(crate) fn tab_rank(
    config: &SidebarPriorityConfig,
    workspace: Rank,
    id: &str,
    label: &str,
) -> Rank {
    matching_rank(config, |rule| {
        rule.strip_prefix("tab:")
            .is_some_and(|pattern| pattern == id || glob(pattern, label))
    })
    .unwrap_or(workspace)
}

impl super::AppState {
    pub(crate) fn priority_workspace_rank(&self, index: usize) -> Rank {
        let workspace = &self.workspaces[index];
        workspace_rank(
            &self.sidebar_priority,
            &workspace.id,
            &workspace.display_name_from_terminals(&self.terminals),
        )
    }

    pub(crate) fn priority_tab_rank(&self, workspace_index: usize, tab_index: usize) -> Rank {
        let workspace = &self.workspaces[workspace_index];
        let tab = &workspace.tabs[tab_index];
        tab_rank(
            &self.sidebar_priority,
            self.priority_workspace_rank(workspace_index),
            &crate::workspace::public_tab_id_for_number(&workspace.id, tab.number),
            &workspace.tab_display_name(tab_index).unwrap_or_default(),
        )
    }

    /// Stable-sort only the plain block; agents and workspace/tab identities stay put.
    pub(crate) fn sort_priority_pins(&mut self) {
        if self.sidebar_priority.order.is_empty() && self.sidebar_priority.last.is_empty() {
            return;
        }
        let mut ranks = std::collections::HashMap::new();
        for (wi, ws) in self.workspaces.iter().enumerate() {
            for (ti, tab) in ws.tabs.iter().enumerate() {
                ranks.insert(
                    crate::workspace::public_tab_id_for_number(&ws.id, tab.number),
                    self.priority_tab_rank(wi, ti).value,
                );
            }
        }
        let agents = self
            .pinned_tabs
            .iter()
            .take_while(|pin| pin.role.is_some())
            .count();
        self.pinned_tabs[agents..].sort_by_key(|pin| {
            ranks
                .get(&pin.tab_id)
                .copied()
                .unwrap_or(self.sidebar_priority.order.len() as u32)
        });
    }

    /// A renamed plain pin joins the end of its new group, preserving peer order.
    pub(crate) fn priority_renamed_pins(&mut self, ids: &[String]) {
        if self.sidebar_priority.order.is_empty() && self.sidebar_priority.last.is_empty() {
            return;
        }
        let mut moved = Vec::new();
        self.pinned_tabs.retain(|pin| {
            if pin.role.is_none() && ids.contains(&pin.tab_id) {
                moved.push(pin.clone());
                false
            } else {
                true
            }
        });
        self.pinned_tabs.extend(moved);
        self.sort_priority_pins();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Pure state ranking/partition algorithm guards stable ties, agent isolation,
    /// rename insertion and unchanged positional identity under adversarial state.
    #[test]
    fn priority_pins_preserve_identity_and_agent_order() {
        let mut state = super::super::AppState::test_with_adversarial_identity_state();
        state.pinned_tabs.clear();
        let ids = state.workspaces[0]
            .tabs
            .iter()
            .take(3)
            .map(|tab| {
                crate::workspace::public_tab_id_for_number(&state.workspaces[0].id, tab.number)
            })
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 3);
        state.workspaces[0].tabs[0].set_custom_name("slow".into());
        state.workspaces[0].tabs[1].set_custom_name("fast".into());
        state.workspaces[0].tabs[2].set_custom_name("fast".into());
        state.pin_tab(ids[0].clone(), 0);
        state.pin_tab(ids[1].clone(), 0);
        state.assert_invariants_for_test();
        state.sidebar_priority.order = vec!["tab:fast".into()];
        state.sort_priority_pins();
        assert_eq!(
            state
                .pinned_tabs
                .iter()
                .map(|pin| &pin.tab_id)
                .collect::<Vec<_>>(),
            [&ids[1], &ids[0]]
        );
        state.assert_invariants_for_test();
        state.pin_tab(ids[2].clone(), 100);
        assert_eq!(
            state
                .pinned_tabs
                .iter()
                .map(|pin| &pin.tab_id)
                .collect::<Vec<_>>(),
            [&ids[1], &ids[2], &ids[0]]
        );
        state.assert_invariants_for_test();
        state.workspaces[0].tabs[0].set_custom_name("fast".into());
        state.priority_renamed_pins(&[ids[0].clone()]);
        assert_eq!(state.pinned_tab_index(&ids[0]), Some(2));
        state.set_tab_role(&ids[2], Some(crate::api::schema::TabRole::Agent));
        state.sort_priority_pins();
        assert_eq!(state.pinned_tabs[0].tab_id, ids[2]);
        state.assert_invariants_for_test();
        state.sidebar_priority = SidebarPriorityConfig::default();
        let before = state.pinned_tabs.clone();
        state.sort_priority_pins();
        assert_eq!(state.pinned_tabs, before);
        state.assert_invariants_for_test();
    }

    /// Pure ranking algorithm: independent table guards rule precedence, glob edges and fallback.
    #[test]
    fn priority_rank_rules() {
        let config = SidebarPriorityConfig {
            order: vec![
                "Recruiting".into(),
                "tab:factory*throughput".into(),
                "w2".into(),
            ],
            last: vec!["rails".into(), "tab:*parked*".into()],
        };
        for (id, label, rank, parked) in [
            ("w1", "RECRUITING", 0, false),
            ("w2", "other", 2, false),
            ("w3", "factory throughput", 3, false),
            ("w4", "Rails", 4, true),
        ] {
            assert_eq!(
                workspace_rank(&config, id, label),
                Rank {
                    value: rank,
                    parked
                }
            );
        }
        let by_id = SidebarPriorityConfig {
            order: vec!["tab:w4:t1".into()],
            last: Vec::new(),
        };
        assert_eq!(
            tab_rank(
                &by_id,
                Rank {
                    value: 9,
                    parked: false
                },
                "w4:t1",
                "unrelated"
            )
            .value,
            0
        );
        assert_eq!(workspace_rank(&by_id, "w4", "w4:t1").value, 1);
        let ws = workspace_rank(&config, "w4", "rails");
        assert_eq!(
            tab_rank(&config, ws, "w4:t1", "Factory big Throughput").value,
            1
        );
        assert_eq!(tab_rank(&config, ws, "w4:t1", "other"), ws);
        assert_eq!(tab_rank(&config, ws, "w4:t1", "parked task").value, 5);
        assert_eq!(
            workspace_rank(&SidebarPriorityConfig::default(), "w1", "rails").value,
            0
        );
        for (pattern, value, expected) in [
            ("*", "", true),
            ("a*b", "ab", true),
            ("a*b", "axbc", false),
            ("a**b", "ab", true),
            ("a*b*c", "ac", false),
        ] {
            assert_eq!(glob(pattern, value), expected);
        }
    }
}
