use tauri::State;
use writ_plugin::transform::TransformDescriptor;

use crate::state::AppState;

#[tauri::command]
pub fn list_transforms(state: State<'_, AppState>) -> Result<Vec<TransformDescriptor>, String> {
    let registry = state
        .transforms
        .read()
        .map_err(|e| format!("transform registry poisoned: {e}"))?;
    Ok(registry.list())
}

#[tauri::command]
pub fn apply_transform(
    state: State<'_, AppState>,
    transform_id: String,
    input: String,
) -> Result<String, String> {
    let registry = state
        .transforms
        .read()
        .map_err(|e| format!("transform registry poisoned: {e}"))?;
    let transform = registry
        .get(&transform_id)
        .ok_or_else(|| format!("unknown transform: {transform_id}"))?;
    transform.apply(&input).map_err(|e| e.to_string())
}
