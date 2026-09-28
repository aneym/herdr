use serde::Deserialize;

/// `[ui.factory]`: the factory overlay (fork feature, 2026-09-28). Off by default, so a
/// config without this table draws exactly as before.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct FactoryUiConfig {
    /// Group tagged tabs, draw host badges and allow the detail panel. Client-side.
    pub enabled: bool,
    /// JSON overlay document the server polls. `~` expands to $HOME. Empty disables polling.
    pub overlay_file: String,
    /// Fixed width in columns of the detail panel beside the sidebar.
    pub panel_width: u16,
}

pub const DEFAULT_FACTORY_OVERLAY_FILE: &str = "~/.agent-rails/herdr/overlay.json";

impl Default for FactoryUiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            overlay_file: DEFAULT_FACTORY_OVERLAY_FILE.into(),
            panel_width: 46,
        }
    }
}

impl FactoryUiConfig {
    /// The overlay file path with a leading `~/` expanded, or None when polling is off.
    pub fn overlay_path(&self) -> Option<std::path::PathBuf> {
        let raw = self.overlay_file.trim();
        if raw.is_empty() {
            return None;
        }
        if let Some(rest) = raw.strip_prefix("~/") {
            let home = std::env::var_os("HOME")?;
            return Some(std::path::PathBuf::from(home).join(rest));
        }
        Some(std::path::PathBuf::from(raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_off_with_the_agent_rails_path() {
        let config = FactoryUiConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.panel_width, 46);
        assert!(config.overlay_path().unwrap().ends_with(".agent-rails/herdr/overlay.json"));
    }

    #[test]
    fn empty_path_disables_polling() {
        let config = FactoryUiConfig {
            overlay_file: "  ".into(),
            ..FactoryUiConfig::default()
        };
        assert_eq!(config.overlay_path(), None);
    }
}
