use crate::commands::config::persist_config;
use crate::state::AppState;
use tauri::State;
use writ_core::config::SpellingConfig;
use writ_lint::{check, dialect_from_str, LintConfig, LintResult};

/// Maps the persisted spelling config to the engine's per-call config.
fn lint_config_from(spelling: &SpellingConfig) -> LintConfig {
    LintConfig {
        dialect: dialect_from_str(&spelling.dialect),
        ignored_words: spelling.ignored_words.clone(),
    }
}

/// Adds `word` to the ignore list, persisting through the same watcher-safe
/// path as `update_config`. Returns whether the list changed.
fn add_ignored_word_to_state(state: &AppState, word: String) -> Result<bool, String> {
    let mut config = {
        let current = state.config.lock().map_err(|e| e.to_string())?;
        current.clone()
    };

    if !config.spelling.add_ignored_word(word) {
        return Ok(false);
    }

    persist_config(state, &config)?;

    let mut current = state.config.lock().map_err(|e| e.to_string())?;
    *current = config;
    Ok(true)
}

/// Checks `text` for spelling and mechanical mistakes using the user's
/// configured dialect and ignore list. Runs the (CPU-bound) lint on a blocking
/// thread so the async runtime is never stalled.
#[tauri::command]
pub async fn check_spelling(
    state: State<'_, AppState>,
    text: String,
) -> Result<Vec<LintResult>, String> {
    let config = {
        let current = state.config.lock().map_err(|e| e.to_string())?;
        lint_config_from(&current.spelling)
    };

    tauri::async_runtime::spawn_blocking(move || check(&text, &config))
        .await
        .map_err(|e| e.to_string())
}

/// Adds `word` to the ignore list and persists the config. The write records
/// the config filename in the watcher ignore set first, so the app's own write
/// is not re-surfaced as an external change.
#[tauri::command]
pub fn spelling_add_ignored_word(state: State<'_, AppState>, word: String) -> Result<(), String> {
    add_ignored_word_to_state(&state, word).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    // `AppState::initialize` reads `WRIT_DATA_DIR`; serialize the env mutation so
    // parallel tests in this module never race on it.
    static ENV_LOCK: StdMutex<()> = StdMutex::new(());

    fn with_state<F: FnOnce(&AppState)>(f: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("WRIT_DATA_DIR", dir.path());
        let state = AppState::initialize().expect("init app state");
        std::env::remove_var("WRIT_DATA_DIR");
        f(&state);
    }

    #[test]
    fn lint_config_maps_dialect_and_passes_ignore_list() {
        let spelling = SpellingConfig {
            enabled: true,
            dialect: "british".to_string(),
            ignored_words: vec!["writ".to_string()],
        };
        let cfg = lint_config_from(&spelling);
        assert!(matches!(cfg.dialect, writ_lint::Dialect::British));
        assert_eq!(cfg.ignored_words, vec!["writ".to_string()]);
    }

    #[test]
    fn lint_config_unknown_dialect_falls_back_to_american() {
        let spelling = SpellingConfig {
            dialect: "klingon".to_string(),
            ..SpellingConfig::default()
        };
        let cfg = lint_config_from(&spelling);
        assert!(matches!(cfg.dialect, writ_lint::Dialect::American));
    }

    #[test]
    fn check_spelling_flags_a_misspelling_through_state() {
        with_state(|state| {
            let config = {
                let current = state.config.lock().unwrap();
                lint_config_from(&current.spelling)
            };
            let result = tauri::async_runtime::block_on(async {
                tauri::async_runtime::spawn_blocking(move || check("I recieve it.", &config))
                    .await
                    .unwrap()
            });
            assert!(result.iter().any(|r| r.kind == "Spelling"));
        });
    }

    #[test]
    fn check_spelling_respects_the_configured_ignore_list() {
        with_state(|state| {
            add_ignored_word_to_state(state, "recieve".to_string()).unwrap();
            let config = {
                let current = state.config.lock().unwrap();
                lint_config_from(&current.spelling)
            };
            let result = check("I recieve it.", &config);
            assert!(
                result.iter().all(|r| r.kind != "Spelling"),
                "ignored word should not be flagged: {result:?}"
            );
        });
    }

    #[test]
    fn add_ignored_word_persists_and_dedups() {
        with_state(|state| {
            assert!(add_ignored_word_to_state(state, "recieve".to_string()).unwrap());
            // Second add is a no-op.
            assert!(!add_ignored_word_to_state(state, "recieve".to_string()).unwrap());

            // In-memory state carries the word.
            {
                let current = state.config.lock().unwrap();
                assert_eq!(current.spelling.ignored_words, vec!["recieve".to_string()]);
            }

            // And it survives a fresh read from disk.
            let on_disk = state.config_store.read().expect("read config");
            assert_eq!(on_disk.spelling.ignored_words, vec!["recieve".to_string()]);
        });
    }
}
