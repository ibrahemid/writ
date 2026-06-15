//! In-app updater IPC surface.
//!
//! The updater *mechanism* lives here (checking, downloading, installing via
//! `tauri-plugin-updater`); the *policy* — which phase transitions are legal —
//! lives in [`writ_core::update`]. Every observed step is fed through the
//! phase machine and mirrored to the frontend as a `writ://update-status`
//! event carrying the full [`UpdatePhase`].

use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;
use writ_core::update::{UpdateEvent, UpdatePhase};

/// File under the Writ data dir holding the epoch-ms of the last silent
/// update check. Kept out of `config.toml` so interval bookkeeping never
/// rewrites the user's editable config or races the frontend's config writes.
const LAST_CHECK_FILE: &str = ".update_check";

/// Current time as epoch milliseconds, or `0` if the clock is before the Unix
/// epoch (which makes the gate treat the check as due rather than panicking).
pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Reads the last silent-check timestamp, or `None` if it has never run or
/// the marker is missing/unparseable.
pub fn read_last_check_ms(writ_dir: &Path) -> Option<u64> {
    std::fs::read_to_string(writ_dir.join(LAST_CHECK_FILE))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
}

/// Records the silent-check timestamp. Best-effort: a write failure only means
/// the next launch may check again, so it is logged, not propagated.
pub fn write_last_check_ms(writ_dir: &Path, ms: u64) {
    if let Err(e) = std::fs::write(writ_dir.join(LAST_CHECK_FILE), ms.to_string()) {
        tracing::debug!(error = %e, "failed to record last update-check time");
    }
}

use crate::events::{emit_event, WritFrontendEvent};
use crate::poison::recover_poison;
use crate::state::AppState;

const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(100);

/// Checks for an update and notifies the frontend.
///
/// `user_initiated` controls visibility: a manual check surfaces every
/// outcome (checking, up-to-date, failed). The silent startup check stays
/// quiet on "no update" and on failure — pre-launch the endpoint 404s by
/// design, and a missing update server must never spam logs or flash a
/// banner. Only a genuinely available update is surfaced from the silent
/// path.
#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<(), String> {
    run_update_check(app, true).await;
    Ok(())
}

/// Downloads and installs the available update, streaming throttled progress
/// to the frontend, then stages the app for restart.
#[tauri::command]
pub async fn download_and_install_update(app: AppHandle) -> Result<(), String> {
    let updater = build_updater(&app).map_err(|e| sanitize_update_error(&e.to_string()))?;

    advance(&app, UpdateEvent::CheckStarted);
    let update = match updater.check().await {
        Ok(Some(update)) => {
            advance(
                &app,
                UpdateEvent::UpdateFound {
                    version: update.version.clone(),
                },
            );
            update
        }
        Ok(None) => {
            let phase = advance(&app, UpdateEvent::NoUpdate);
            emit_phase(&app, &phase);
            return Ok(());
        }
        Err(e) => {
            let phase = advance(
                &app,
                UpdateEvent::Errored {
                    message: sanitize_update_error(&e.to_string()),
                },
            );
            emit_phase(&app, &phase);
            return Ok(());
        }
    };

    let downloading = advance(&app, UpdateEvent::DownloadStarted { total: None });
    emit_phase(&app, &downloading);

    let progress_app = app.clone();
    let mut downloaded: u64 = 0;
    let mut last_emit = Instant::now();

    let result = update
        .download_and_install(
            |chunk_len, content_length| {
                downloaded += chunk_len as u64;
                if last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
                    last_emit = Instant::now();
                    let phase = advance(
                        &progress_app,
                        UpdateEvent::DownloadProgressed {
                            downloaded,
                            total: content_length,
                        },
                    );
                    emit_phase(&progress_app, &phase);
                }
            },
            || {
                let phase = advance(&progress_app, UpdateEvent::DownloadCompleted);
                emit_phase(&progress_app, &phase);
            },
        )
        .await;

    match result {
        Ok(()) => {
            let phase = advance(&app, UpdateEvent::InstallCompleted);
            emit_phase(&app, &phase);
            tracing::info!("update installed; awaiting restart");
        }
        Err(e) => {
            let phase = advance(
                &app,
                UpdateEvent::Errored {
                    message: sanitize_update_error(&e.to_string()),
                },
            );
            emit_phase(&app, &phase);
            tracing::warn!("update install failed");
        }
    }
    Ok(())
}

/// Dismisses the update surface, returning the phase to idle.
#[tauri::command]
pub fn dismiss_update(app: AppHandle) {
    let phase = advance(&app, UpdateEvent::Dismissed);
    emit_phase(&app, &phase);
}

/// Relaunches the app to apply a staged update.
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    tracing::info!("restarting to apply update");
    app.restart();
}

/// Runs a check, advancing the phase machine and emitting status per the
/// `user_initiated` visibility policy. Shared by the IPC command and the
/// silent startup check in `lib.rs`.
pub async fn run_update_check(app: AppHandle, user_initiated: bool) {
    let checking = advance(&app, UpdateEvent::CheckStarted);
    if user_initiated {
        emit_phase(&app, &checking);
    }

    let updater = match build_updater(&app) {
        Ok(updater) => updater,
        Err(e) => {
            finish_check_failure(&app, user_initiated, &e.to_string());
            return;
        }
    };

    match updater.check().await {
        Ok(Some(update)) => {
            tracing::info!(version = %update.version, "update available");
            let phase = advance(
                &app,
                UpdateEvent::UpdateFound {
                    version: update.version,
                },
            );
            emit_phase(&app, &phase);
        }
        Ok(None) => {
            tracing::debug!("no update available");
            let phase = advance(&app, UpdateEvent::NoUpdate);
            if user_initiated {
                emit_phase(&app, &phase);
            } else {
                advance(&app, UpdateEvent::Dismissed);
            }
        }
        Err(e) => finish_check_failure(&app, user_initiated, &e.to_string()),
    }
}

fn finish_check_failure(app: &AppHandle, user_initiated: bool, raw: &str) {
    let message = sanitize_update_error(raw);
    let phase = advance(app, UpdateEvent::Errored { message });
    if user_initiated {
        emit_phase(app, &phase);
        tracing::warn!("update check failed");
    } else {
        advance(app, UpdateEvent::Dismissed);
        tracing::debug!("silent update check failed");
    }
}

fn advance(app: &AppHandle, event: UpdateEvent) -> UpdatePhase {
    let state = app.state::<AppState>();
    let mut guard = recover_poison(state.update_phase.lock(), "commands::update::advance");
    match guard.apply(event) {
        Ok(next) => {
            *guard = next.clone();
            next
        }
        Err(illegal) => {
            tracing::warn!(%illegal, "ignored illegal update transition");
            guard.clone()
        }
    }
}

fn emit_phase(app: &AppHandle, phase: &UpdatePhase) {
    if let Err(e) = emit_event(app, WritFrontendEvent::UpdateStatus(phase.clone())) {
        tracing::warn!(error = %e, "failed to emit update status");
    }
}

/// Builds an [`Updater`](tauri_plugin_updater::Updater) from the bundled
/// config. In debug builds only, `WRIT_UPDATER_ENDPOINT` (and optional
/// `WRIT_UPDATER_PUBKEY`) override the endpoint for the local test loop.
///
/// The override branch is compiled out of release builds: a release binary
/// must never let an environment variable redirect the update source or the
/// verification key, which would be a supply-chain hole.
fn build_updater(app: &AppHandle) -> tauri_plugin_updater::Result<tauri_plugin_updater::Updater> {
    #[cfg(debug_assertions)]
    {
        if let Ok(endpoint) = std::env::var("WRIT_UPDATER_ENDPOINT") {
            match url::Url::parse(&endpoint) {
                Ok(parsed) => {
                    let mut builder = app.updater_builder().endpoints(vec![parsed])?;
                    if let Ok(pubkey) = std::env::var("WRIT_UPDATER_PUBKEY") {
                        builder = builder.pubkey(pubkey);
                    }
                    tracing::warn!("using debug-only updater endpoint override");
                    return builder.build();
                }
                Err(e) => {
                    tracing::warn!(error = %e, "invalid WRIT_UPDATER_ENDPOINT; ignoring override");
                }
            }
        }
    }
    app.updater()
}

/// Redacts URLs from an updater error string so endpoints (which may later
/// carry signed query parameters or tokens) never reach logs or the UI.
///
/// Plain, secret-free messages pass through with whitespace collapsed. An
/// empty result falls back to a generic message.
pub fn sanitize_update_error(raw: &str) -> String {
    const REDACTED: &str = "<redacted-url>";

    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while !rest.is_empty() {
        if rest.starts_with("http://") || rest.starts_with("https://") {
            out.push_str(REDACTED);
            let end = rest
                .find(|c: char| {
                    c.is_whitespace() || matches!(c, '(' | ')' | '"' | '\'' | '<' | '>' | ',')
                })
                .unwrap_or(rest.len());
            rest = &rest[end..];
        } else {
            let mut chars = rest.chars();
            let c = chars.next().expect("rest is non-empty");
            out.push(c);
            rest = chars.as_str();
        }
    }

    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        "Update failed.".to_string()
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_https_endpoint_but_keeps_status() {
        let out = sanitize_update_error(
            "failed to fetch https://github.com/ibrahemid/writ/releases/latest/download/latest.json: status 404",
        );
        assert!(!out.contains("github.com"), "url leaked: {out}");
        assert!(!out.contains("https://"), "scheme leaked: {out}");
        assert!(out.contains("404"), "lost the useful detail: {out}");
    }

    #[test]
    fn redacts_url_wrapped_in_parens() {
        let out = sanitize_update_error("error sending request for url (https://example.com/x/y)");
        assert!(!out.contains("example.com"), "url leaked: {out}");
        assert!(out.contains("error sending request"));
    }

    #[test]
    fn plain_message_passes_through() {
        let out = sanitize_update_error("signature verification failed");
        assert_eq!(out, "signature verification failed");
    }

    #[test]
    fn whitespace_only_becomes_generic() {
        let out = sanitize_update_error("   \n  ");
        assert_eq!(out, "Update failed.");
    }
}
