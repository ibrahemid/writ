use crate::poison::recover_poison;
use crate::state::AppState;
use tauri::State;
use writ_core::recovery::RecoveredBuffer;

/// Returns the notes whose text this launch took from the unsaved-text
/// journal (ADR-044), then clears the list.
///
/// Returns an empty list when the journal held nothing.
#[tauri::command]
pub fn get_recovered_buffers(state: State<'_, AppState>) -> Result<Vec<RecoveredBuffer>, String> {
    let mut recovered = recover_poison(
        state.recovered_buffers.lock(),
        "commands::recovery::get_recovered_buffers",
    );
    Ok(std::mem::take(&mut *recovered))
}
