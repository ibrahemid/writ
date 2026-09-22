//! `[apps]` configuration section, and the one list of apps (ADR-042).
//!
//! Chat, Rewrite and Connected programs keep the switches they already had
//! (`ai.chat.enabled`, `ai.rewrite.enabled`, `mcp.enabled`); this table holds
//! the three that had none. [`super::WritConfig::is_app_on`] reads all six.

use serde::{Deserialize, Serialize};

/// One app, in the order Settings lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppId {
    /// The chat pane (`ai.chat.enabled`).
    Chat,
    /// Rewrite on a selection (`ai.rewrite.enabled`).
    Rewrite,
    /// The MCP server and its Activity panel (`mcp.enabled`).
    Programs,
    /// The panel beside the file (`apps.connections`).
    Connections,
    /// The folder graph and the nearby-files graph (`apps.graph`).
    Graph,
    /// The tags sidebar section and its filter (`apps.tags`).
    Tags,
}

impl AppId {
    /// Every app, in Settings order.
    pub const ALL: [AppId; 6] = [
        AppId::Chat,
        AppId::Rewrite,
        AppId::Programs,
        AppId::Connections,
        AppId::Graph,
        AppId::Tags,
    ];
}

/// The switches for the apps that have no section of their own.
///
/// [`Default`] is a fresh install: everything off. A file written before this
/// table existed reads [`AppsConfig::existing`] instead, because those apps
/// were on for everybody who had one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AppsConfig {
    /// The connections panel.
    #[serde(default)]
    pub connections: bool,
    /// The folder graph and nearby files.
    #[serde(default)]
    pub graph: bool,
    /// The tags section.
    #[serde(default)]
    pub tags: bool,
}

impl AppsConfig {
    /// What a config written before `[apps]` existed had: all three on.
    pub fn existing() -> Self {
        Self {
            connections: true,
            graph: true,
            tags: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SidebarSection, WritConfig};

    fn on(config: &WritConfig) -> Vec<AppId> {
        config.apps_on().into_iter().collect()
    }

    #[test]
    fn a_fresh_config_has_every_app_off() {
        assert!(on(&WritConfig::default()).is_empty());
    }

    #[test]
    fn a_fresh_config_hides_the_inbox_and_recent_sections() {
        let fresh = WritConfig::default();
        assert_eq!(
            fresh.sidebar.hidden,
            vec![SidebarSection::Inbox, SidebarSection::Recent]
        );
    }

    #[test]
    fn a_config_written_before_apps_keeps_connections_graph_and_tags() {
        let config = WritConfig::parse("[editor]\nfont_size = 14\n").unwrap();
        assert_eq!(
            on(&config),
            vec![AppId::Connections, AppId::Graph, AppId::Tags]
        );
        assert!(config.sidebar.hidden.is_empty());
    }

    #[test]
    fn existing_ai_and_mcp_switches_map_onto_chat_rewrite_and_programs() {
        let config = WritConfig::parse(
            "[ai]\nprovider = \"ollama\"\n[ai.chat]\nenabled = true\n[ai.rewrite]\nenabled = true\n[mcp]\nenabled = true\n[apps]\n",
        )
        .unwrap();
        assert_eq!(
            on(&config),
            vec![AppId::Chat, AppId::Rewrite, AppId::Programs]
        );
    }

    #[test]
    fn a_hidden_tags_section_becomes_tags_off() {
        let config = WritConfig::parse("[sidebar]\nhidden = [\"tags\", \"recent\"]\n").unwrap();
        assert!(!config.is_app_on(AppId::Tags));
        assert!(config.is_app_on(AppId::Connections));
        assert!(config.is_app_on(AppId::Graph));
        assert_eq!(config.sidebar.hidden, vec![SidebarSection::Recent]);
    }

    #[test]
    fn settling_twice_changes_nothing() {
        let mut config =
            WritConfig::parse("[sidebar]\nhidden = [\"tags\"]\n[apps]\ntags = true\n").unwrap();
        let once = config.clone();
        config.settle_apps();
        assert_eq!(config, once);
    }

    #[test]
    fn an_apps_table_is_read_as_written() {
        let config = WritConfig::parse("[apps]\ngraph = true\n").unwrap();
        assert_eq!(on(&config), vec![AppId::Graph]);
    }

    #[test]
    fn set_app_writes_each_apps_own_key() {
        let mut config = WritConfig::default();
        for app in AppId::ALL {
            config.set_app(app, true);
            assert!(config.is_app_on(app), "{app:?} did not turn on");
        }
        assert!(config.ai.chat.enabled);
        assert!(config.ai.rewrite.enabled);
        assert!(config.mcp.enabled);
        assert_eq!(config.apps, AppsConfig::existing());
        for app in AppId::ALL {
            config.set_app(app, false);
        }
        assert!(on(&config).is_empty());
    }

    #[test]
    fn switches_survive_a_round_trip_through_toml() {
        let mut config = WritConfig::default();
        config.set_app(AppId::Tags, true);
        config.set_app(AppId::Chat, true);
        let back = WritConfig::parse(&toml::to_string(&config).unwrap()).unwrap();
        assert_eq!(back, config);
    }

    #[test]
    fn app_ids_use_their_config_words() {
        let words: Vec<String> = AppId::ALL
            .iter()
            .map(|app| serde_json::to_string(app).unwrap())
            .collect();
        assert_eq!(
            words,
            [
                "\"chat\"",
                "\"rewrite\"",
                "\"programs\"",
                "\"connections\"",
                "\"graph\"",
                "\"tags\""
            ]
        );
    }
}
