use crate::commands::config::persist_config;
use crate::state::AppState;
use tauri::State;
use writ_lint::{check, dialect_from_str, LintConfig, LintResult};

/// Checks `text` for spelling and mechanical mistakes using the user's
/// configured dialect and ignore list. Runs the (CPU-bound) lint on a blocking
/// thread so the async runtime is never stalled.
#[tauri::command]
pub async fn check_spelling(
    state: State<'_, AppState>,
    text: String,
) -> Result<Vec<LintResult>, String> {
    let (dialect, ignored_words) = {
        let config = state.config.lock().map_err(|e| e.to_string())?;
        (
            config.spelling.dialect.clone(),
            config.spelling.ignored_words.clone(),
        )
    };

    let config = LintConfig {
        dialect: dialect_from_str(&dialect),
        ignored_words,
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
    let mut config = {
        let current = state.config.lock().map_err(|e| e.to_string())?;
        current.clone()
    };

    if !config.spelling.add_ignored_word(word) {
        return Ok(());
    }

    persist_config(&state, &config)?;

    let mut current = state.config.lock().map_err(|e| e.to_string())?;
    *current = config;
    Ok(())
}

#[cfg(test)]
mod tests {
    use writ_core::config::SpellingConfig;

    #[test]
    fn add_ignored_word_dedups_and_reports_change() {
        let mut config = SpellingConfig::default();
        assert!(config.add_ignored_word("recieve".to_string()));
        assert!(!config.add_ignored_word("recieve".to_string()));
        assert_eq!(config.ignored_words, vec!["recieve".to_string()]);
    }

    #[test]
    fn add_ignored_word_keeps_list_sorted() {
        let mut config = SpellingConfig::default();
        config.add_ignored_word("writ".to_string());
        config.add_ignored_word("tauri".to_string());
        assert_eq!(
            config.ignored_words,
            vec!["tauri".to_string(), "writ".to_string()]
        );
    }
}
