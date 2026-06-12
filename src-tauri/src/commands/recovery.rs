use crate::poison::recover_poison;
use crate::state::AppState;
use tauri::State;
use writ_core::recovery::RecoveredBuffer;

/// Returns the list of buffers restored from a crash snapshot on this
/// launch, then clears the list.
///
/// Returns an empty list when the previous shutdown was clean.
#[tauri::command]
pub fn get_recovered_buffers(state: State<'_, AppState>) -> Result<Vec<RecoveredBuffer>, String> {
    let mut recovered = recover_poison(
        state.recovered_buffers.lock(),
        "commands::recovery::get_recovered_buffers",
    );
    Ok(std::mem::take(&mut *recovered))
}
