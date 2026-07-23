//! `[spelling]` configuration section.
//!
//! Local spell check is off by default and opt-in per install. When enabled,
//! the editor checks prose (never code fences, inline code, or URLs) against
//! the chosen English dialect and flags a small set of mechanical mistakes.
//! Every field has a serde default so existing configs upgrade cleanly.

use serde::{Deserialize, Serialize};

fn default_enabled() -> bool {
    false
}

fn default_dialect() -> String {
    "american".to_string()
}

fn default_ignored_words() -> Vec<String> {
    Vec::new()
}

/// Spell-check surface configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpellingConfig {
    /// Master switch. When `false`, no checking runs, no decorations are
    /// drawn, and the status-bar chip is hidden.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// English dialect the checker validates against. Accepted values are
    /// `american`, `british`, `canadian`, and `australian`; anything else is
    /// treated as `american` by the engine.
    #[serde(default = "default_dialect")]
    pub dialect: String,
    /// Words the user has chosen never to flag. Stored deduplicated and sorted.
    #[serde(default = "default_ignored_words")]
    pub ignored_words: Vec<String>,
}

impl Default for SpellingConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            dialect: default_dialect(),
            ignored_words: default_ignored_words(),
        }
    }
}

impl SpellingConfig {
    /// Adds `word` to the ignore list, then deduplicates and sorts it so the
    /// persisted form is stable regardless of insertion order. Returns `true`
    /// when the list changed.
    pub fn add_ignored_word(&mut self, word: String) -> bool {
        let word = word.trim().to_string();
        if word.is_empty() || self.ignored_words.iter().any(|w| w == &word) {
            return false;
        }
        self.ignored_words.push(word);
        self.ignored_words.sort();
        self.ignored_words.dedup();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off_and_american() {
        let c = SpellingConfig::default();
        assert!(!c.enabled);
        assert_eq!(c.dialect, "american");
        assert!(c.ignored_words.is_empty());
    }

    #[test]
    fn empty_table_yields_defaults() {
        let c: SpellingConfig = toml::from_str("").unwrap();
        assert_eq!(c, SpellingConfig::default());
    }

    #[test]
    fn partial_table_keeps_other_defaults() {
        let c: SpellingConfig = toml::from_str("enabled = true").unwrap();
        assert!(c.enabled);
        assert_eq!(c.dialect, "american");
        assert!(c.ignored_words.is_empty());
    }

    #[test]
    fn round_trips_through_toml() {
        let mut c = SpellingConfig {
            enabled: true,
            dialect: "british".to_string(),
            ignored_words: vec!["writ".to_string(), "tauri".to_string()],
        };
        c.ignored_words.sort();
        let s = toml::to_string(&c).unwrap();
        let back: SpellingConfig = toml::from_str(&s).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn add_ignored_word_dedups_and_sorts() {
        let mut c = SpellingConfig::default();
        assert!(c.add_ignored_word("zeta".to_string()));
        assert!(c.add_ignored_word("alpha".to_string()));
        assert!(!c.add_ignored_word("alpha".to_string()));
        assert!(!c.add_ignored_word("  ".to_string()));
        assert_eq!(
            c.ignored_words,
            vec!["alpha".to_string(), "zeta".to_string()]
        );
    }
}
