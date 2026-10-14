use std::collections::HashMap;
use std::path::Path;

use tauri::{AppHandle, Runtime};
use writ_core::prompt::pack::ContextPack;
use writ_storage::context_pack::{build_context_pack, LiveText, PackSource};
use writ_storage::notes_index::NotesIndexStore;

use crate::commands::offload::{run_blocking, ERR_COMMAND_STOPPED};

/// Off the main thread: the estimate walks the whole document, which can be a
/// file in the large tiers (EP-14).
#[tauri::command(async)]
pub fn prompt_estimate_tokens(text: String) -> usize {
    writ_core::prompt::estimate_tokens(&text)
}

#[tauri::command]
pub fn prompt_scan_placeholders(text: String) -> Vec<String> {
    writ_core::prompt::scan_placeholders(&text)
}

#[tauri::command]
pub fn prompt_fill_placeholders(text: String, values: HashMap<String, String>) -> String {
    writ_core::prompt::fill_placeholders(&text, &values)
}

/// Several files as one prompt (a context pack): `source`'s files, with the
/// text the open tabs in `live` hold in place of what their files hold. Off
/// the main thread: it reads up to a hundred files.
#[tauri::command]
pub async fn prompt_context_pack<R: Runtime>(
    app: AppHandle<R>,
    source: PackSource,
    live: Vec<LiveText>,
) -> Result<ContextPack, String> {
    run_blocking(app, ERR_COMMAND_STOPPED, move |state| {
        prompt_context_pack_inner(&state.notes_index, &state.notes_root(), &source, &live)
    })
    .await
}

/// [`prompt_context_pack`]'s work, for the tests and the shell budget.
pub fn prompt_context_pack_inner(
    index: &NotesIndexStore,
    notes_root: &Path,
    source: &PackSource,
    live: &[LiveText],
) -> Result<ContextPack, String> {
    build_context_pack(index, notes_root, source, live).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_returns_zero_for_empty_text() {
        assert_eq!(prompt_estimate_tokens(String::new()), 0);
    }

    #[test]
    fn estimate_tokens_returns_positive_for_prose() {
        let est = prompt_estimate_tokens("The quick brown fox jumps over the lazy dog.".into());
        assert!(est > 0);
    }

    #[test]
    fn scan_placeholders_returns_ordered_unique_names() {
        let names = prompt_scan_placeholders("{{b}} {{a}} {{b}}".into());
        assert_eq!(names, vec!["b".to_string(), "a".to_string()]);
    }

    #[test]
    fn fill_placeholders_substitutes_supplied_values() {
        let values = HashMap::from([("name".to_string(), "Writ".to_string())]);
        let filled = prompt_fill_placeholders("hello {{name}}".into(), values);
        assert_eq!(filled, "hello Writ");
    }

    #[test]
    fn fill_placeholders_leaves_unknown_slots_intact() {
        let filled = prompt_fill_placeholders("hello {{name}}".into(), HashMap::new());
        assert_eq!(filled, "hello {{name}}");
    }
}
