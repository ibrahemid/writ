//! IPC for a program's held write (ADR-053): the main window's answer.

use tauri::{AppHandle, Manager, Runtime, Webview};

use crate::write_review::{answer_inner, ReviewAsks};

/// IPC: the person applied or discarded the held write `token` names.
///
/// Off the main thread: the answer is a file written to the data folder
/// (ADR-047).
#[tauri::command]
pub async fn review_answer<R: Runtime>(
    app: AppHandle<R>,
    webview: Webview<R>,
    token: String,
    apply: bool,
) -> Result<(), String> {
    let window = webview.label().to_string();
    let handle = app.clone();
    super::offload::run_blocking(app, super::offload::ERR_COMMAND_STOPPED, move |state| {
        answer_inner(state, &handle.state::<ReviewAsks>(), &window, &token, apply)
            .map_err(|error| error.to_string())
    })
    .await
}
