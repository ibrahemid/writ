//! IPC for the open-file context (ADR-053): the main window's answer to a
//! `get_context` ask.

use tauri::{AppHandle, Manager, Runtime, Webview};

use crate::open_context::{answer_inner, ContextAsks, WindowContext};

/// IPC: the main window says which tab is in front and what is selected, for
/// the ask holding `token`.
///
/// Off the main thread: the answer is a file written to the data folder
/// (ADR-047).
#[tauri::command]
pub async fn context_answer<R: Runtime>(
    app: AppHandle<R>,
    webview: Webview<R>,
    token: String,
    reply: WindowContext,
) -> Result<(), String> {
    let window = webview.label().to_string();
    let handle = app.clone();
    super::offload::run_blocking(app, super::offload::ERR_COMMAND_STOPPED, move |state| {
        answer_inner(
            state,
            &handle.state::<ContextAsks>(),
            &window,
            &token,
            reply,
        )
        .map_err(|error| error.to_string())
    })
    .await
}
