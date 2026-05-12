use tracing::info;

#[tauri::command]
pub fn report_first_paint(elapsed_ms: f64, mode: String, rust_elapsed_us: Option<u64>) {
    info!(
        mode = %mode,
        elapsed_ms = elapsed_ms,
        rust_elapsed_us = rust_elapsed_us.unwrap_or(0),
        "toggle-latency first paint"
    );
}
