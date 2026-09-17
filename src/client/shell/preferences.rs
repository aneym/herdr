use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(1);

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct ClientRemoteCollapsedGroups {
    pub(super) profile_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) collapsed_groups: Vec<String>,
}

/// Per-endpoint tree chrome: which layers the unified sidebar tree shows, what
/// is folded away, and which spaces are pinned. The fork kept these on the
/// server's `AppState`; 0.9 renders the tree in the client, so they live with
/// the rest of the per-client chrome state.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub(super) struct ClientTreeChromePreferences {
    pub(super) show_spaces: bool,
    pub(super) show_tabs: bool,
    pub(super) show_agents: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) collapsed_spaces: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) collapsed_tabs: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) pinned_spaces: Vec<String>,
    pub(super) show_hidden_spaces: bool,
    pub(super) hidden_spaces_expanded: bool,
    pub(super) automations_expanded: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) collapsed_agent_groups: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) space_order: Vec<String>,
}

impl Default for ClientTreeChromePreferences {
    fn default() -> Self {
        Self {
            show_spaces: true,
            show_tabs: true,
            show_agents: true,
            collapsed_spaces: Vec::new(),
            collapsed_tabs: Vec::new(),
            pinned_spaces: Vec::new(),
            show_hidden_spaces: false,
            hidden_spaces_expanded: false,
            automations_expanded: false,
            collapsed_agent_groups: Vec::new(),
            space_order: Vec::new(),
        }
    }
}

impl ClientTreeChromePreferences {
    pub(super) fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct ClientRemoteTreeChrome {
    pub(super) profile_id: String,
    #[serde(default)]
    pub(super) tree: ClientTreeChromePreferences,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct ClientChromePreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) sidebar_width: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) sidebar_section_split: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) sidebar_collapsed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) agent_panel_sort: Option<crate::config::AgentPanelSortConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) collapsed_groups: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) remote_collapsed_groups: Vec<ClientRemoteCollapsedGroups>,
    #[serde(
        default,
        skip_serializing_if = "ClientTreeChromePreferences::is_default"
    )]
    pub(super) tree: ClientTreeChromePreferences,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) remote_tree: Vec<ClientRemoteTreeChrome>,
}

/// Seed a fresh preferences file from the chrome state the 0.8 session file
/// carried. 0.9 keeps sidebar width, the section split and the collapsed groups
/// per client, and the fork's tree state moved with them; a session file written
/// before the move still holds the values worth keeping.
pub(super) fn migrated_from_session(
    mut preferences: ClientChromePreferences,
    session: Option<crate::persist::SessionSnapshot>,
) -> ClientChromePreferences {
    let Some(session) = session else {
        return preferences;
    };
    preferences.sidebar_width = preferences.sidebar_width.or(session.sidebar_width);
    preferences.sidebar_section_split = preferences
        .sidebar_section_split
        .or(session.sidebar_section_split);
    if preferences.collapsed_groups.is_empty() {
        let mut collapsed = session.collapsed_space_keys.into_iter().collect::<Vec<_>>();
        collapsed.sort();
        preferences.collapsed_groups = collapsed;
    }
    if preferences.tree.is_default() {
        let sorted = |values: std::collections::HashSet<String>| {
            let mut values = values.into_iter().collect::<Vec<_>>();
            values.sort();
            values
        };
        preferences.tree = ClientTreeChromePreferences {
            show_spaces: session.tree_show_spaces,
            show_tabs: session.tree_show_tabs,
            show_agents: session.tree_show_agents,
            collapsed_spaces: sorted(session.tree_collapsed_spaces),
            collapsed_tabs: sorted(session.tree_collapsed_tabs),
            pinned_spaces: sorted(session.tree_pinned_spaces),
            show_hidden_spaces: session.tree_show_hidden_spaces,
            hidden_spaces_expanded: session.hidden_spaces_expanded,
            automations_expanded: session.automations_expanded,
            collapsed_agent_groups: sorted(session.collapsed_agent_group_keys),
            space_order: Vec::new(),
        };
    }
    preferences
}

pub(super) fn path_for_local_endpoint(socket_path: &Path) -> PathBuf {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in socket_path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    crate::config::state_dir()
        .join("client-shell")
        .join(format!("local-{hash:016x}.json"))
}

pub(super) fn load(path: &Path) -> Option<ClientChromePreferences> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

pub(super) fn store(path: &Path, preferences: ClientChromePreferences) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("invalid client shell state path: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create client shell state directory: {error}"))?;
    let content = serde_json::to_vec_pretty(&preferences)
        .map_err(|error| format!("failed to encode client shell state: {error}"))?;
    let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
    let mut temp_name = path
        .file_name()
        .ok_or_else(|| format!("invalid client shell state path: {}", path.display()))?
        .to_os_string();
    temp_name.push(format!(".tmp-{}-{sequence}", std::process::id()));
    let temp_path = parent.join(temp_name);
    std::fs::write(&temp_path, content)
        .map_err(|error| format!("failed to write client shell state: {error}"))?;
    std::fs::rename(&temp_path, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        format!("failed to replace client shell state: {error}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_paths_are_stable_and_distinct() {
        let first = path_for_local_endpoint(Path::new("/run/herdr/one.sock"));
        let again = path_for_local_endpoint(Path::new("/run/herdr/one.sock"));
        let second = path_for_local_endpoint(Path::new("/run/herdr/two.sock"));
        assert_eq!(first, again);
        assert_ne!(first, second);
    }

    fn legacy_session(json: &str) -> crate::persist::SessionSnapshot {
        serde_json::from_str(json).expect("legacy session snapshot")
    }

    #[test]
    fn a_fresh_preferences_file_inherits_the_legacy_session_chrome() {
        let session = legacy_session(
            r#"{
                "version": 1,
                "workspaces": [],
                "active": null,
                "selected": 0,
                "sidebar_width": 31,
                "sidebar_section_split": 0.4,
                "collapsed_space_keys": ["/repo", "/other"],
                "tree_show_tabs": false,
                "tree_pinned_spaces": ["ws_7"],
                "automations_expanded": true
            }"#,
        );

        let migrated = migrated_from_session(ClientChromePreferences::default(), Some(session));

        assert_eq!(migrated.sidebar_width, Some(31));
        assert_eq!(migrated.sidebar_section_split, Some(0.4));
        assert_eq!(migrated.collapsed_groups, ["/other", "/repo"]);
        assert!(!migrated.tree.show_tabs);
        assert!(migrated.tree.show_spaces);
        assert_eq!(migrated.tree.pinned_spaces, ["ws_7"]);
        assert!(migrated.tree.automations_expanded);
    }

    #[test]
    fn existing_preferences_win_over_the_legacy_session() {
        let session = legacy_session(
            r#"{
                "version": 1,
                "workspaces": [],
                "active": null,
                "selected": 0,
                "sidebar_width": 31,
                "collapsed_space_keys": ["/repo"],
                "tree_show_tabs": false
            }"#,
        );
        let existing = ClientChromePreferences {
            sidebar_width: Some(22),
            collapsed_groups: vec!["/kept".into()],
            ..ClientChromePreferences::default()
        };

        let migrated = migrated_from_session(existing, Some(session));

        assert_eq!(migrated.sidebar_width, Some(22));
        assert_eq!(migrated.collapsed_groups, ["/kept"]);
        // The tree block was still at its defaults, so it takes the session's.
        assert!(!migrated.tree.show_tabs);
    }

    #[test]
    fn legacy_preferences_default_remote_collapses() {
        let preferences: ClientChromePreferences =
            serde_json::from_str(r#"{"collapsed_groups":["/repo"]}"#)
                .expect("legacy client chrome preferences");

        assert_eq!(preferences.collapsed_groups, ["/repo"]);
        assert!(preferences.remote_collapsed_groups.is_empty());
    }

    #[test]
    fn concurrent_stores_leave_complete_preferences() {
        let path = std::env::temp_dir().join(format!(
            "herdr-shell-concurrent-preferences-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let writers = (20..28)
            .map(|width| {
                let path = path.clone();
                std::thread::spawn(move || {
                    store(
                        &path,
                        ClientChromePreferences {
                            sidebar_width: Some(width),
                            ..ClientChromePreferences::default()
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        for writer in writers {
            writer.join().expect("preference writer").expect("store");
        }
        assert!(load(&path)
            .and_then(|saved| saved.sidebar_width)
            .is_some_and(|width| (20..28).contains(&width)));
        std::fs::remove_file(path).expect("remove preferences");
    }

    #[test]
    fn repeated_store_replaces_existing_preferences() {
        let path = std::env::temp_dir().join(format!(
            "herdr-shell-preferences-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        store(
            &path,
            ClientChromePreferences {
                sidebar_width: Some(24),
                ..ClientChromePreferences::default()
            },
        )
        .expect("first preference store");
        store(
            &path,
            ClientChromePreferences {
                sidebar_width: Some(32),
                ..ClientChromePreferences::default()
            },
        )
        .expect("replacement preference store");
        assert_eq!(load(&path).and_then(|saved| saved.sidebar_width), Some(32));
        std::fs::remove_file(path).expect("remove preferences");
    }
}
