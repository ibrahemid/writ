//! IPC surface for the AI connection: Rewrite, the key row, consent, the model
//! list and the connection check.
//!
//! The connection itself is [`writ_ai::connection`]: the key store, the HTTP
//! client, the stream, the model list and the probes. These commands read the
//! config, hand it to the connection, and write back what the user decided or
//! emit what the connection streamed. Nothing here sends a request itself.

use tauri::{AppHandle, Manager, State};
use tokio_util::sync::CancellationToken;
use writ_ai::connection::{
    self, endpoint_state_from, needs_key_lookup, prepare_request, run_rewrite_stream, StreamEvent,
};
use writ_core::ai::models::{seed_model, ModelCatalog};
use writ_core::ai::providers::{self, ProviderInfo};
use writ_core::config::AiConfig;
use writ_core::polish::{self, PolishError};

pub use writ_ai::connection::{
    build_client, build_client_with, is_consented, probe_local_at, AiConnectionStatus,
    AiEndpointState, AiState, LocalProbe,
};
pub use writ_ai::keys::{AiKeyState, KeyStore, MemoryKeyStore};

use crate::events::{emit_event, WritFrontendEvent};
use crate::poison::recover_poison;
use crate::state::AppState;

/// The key stored for one account, from the keychain or from this session's
/// memory. The account is the provider id, so the surface asking for it never
/// reaches another provider's credential.
pub(crate) fn key_for(app: &AppHandle, account: &str) -> Option<String> {
    app.state::<AiState>().key(account)
}

/// Whether a key is stored for one account, and whether it is confined to this
/// session's memory.
pub(crate) fn key_state_for(app: &AppHandle, account: &str) -> AiKeyState {
    app.state::<AiState>().key_state(account)
}

/// Stores a provider key. Prefers the OS keychain; on failure holds the key in
/// memory for the session and reports that state. Never writes the key to disk
/// and never returns it.
#[tauri::command]
pub fn ai_set_api_key(
    ai: State<'_, AiState>,
    provider: String,
    key: String,
) -> Result<AiKeyState, String> {
    ai.set_key(&provider, &key)
}

/// Removes a provider key from both the keychain and memory.
#[tauri::command]
pub fn ai_clear_api_key(ai: State<'_, AiState>, provider: String) -> Result<AiKeyState, String> {
    Ok(ai.clear_key(&provider))
}

/// Reports whether a key is set for a provider, without returning it.
#[tauri::command]
pub fn ai_has_api_key(ai: State<'_, AiState>, provider: String) -> Result<AiKeyState, String> {
    Ok(ai.key_state(&provider))
}

/// Reports where the configured endpoint points and what it still needs, so the
/// UI can ask for consent or a key before a rewrite is attempted rather than
/// after it fails.
#[tauri::command]
pub fn ai_endpoint_state(app: AppHandle) -> Result<AiEndpointState, String> {
    let cfg = {
        let state = app.state::<AppState>();
        let guard = recover_poison(state.config.lock(), "commands::ai::ai_endpoint_state");
        guard.ai.clone()
    };
    // A local endpoint needs no key, so do not consult the keychain to answer a
    // question nothing reads: on macOS that alone can raise a password prompt.
    let key_state = if needs_key_lookup(&cfg) {
        key_state_for(&app, &cfg.provider)
    } else {
        AiKeyState {
            is_set: false,
            memory_only: false,
        }
    };
    Ok(endpoint_state_from(&cfg, key_state))
}

/// Records the send notice for the host the connection reaches.
///
/// Rewriting and the chat share one connection, so there is one host to
/// consent to and the caller names nothing. The host is resolved here rather
/// than supplied by the caller, so consent is always stored under the exact
/// string the guard later checks — a client-computed host could never drift
/// out of agreement with the guard. Refuses a local or disallowed endpoint:
/// there is nothing to consent to.
#[tauri::command]
pub fn ai_consent_host(app: AppHandle) -> Result<AiEndpointState, String> {
    let state = app.state::<AppState>();
    let mut config = {
        let guard = recover_poison(state.config.lock(), "commands::ai::ai_consent_host");
        guard.clone()
    };

    let target =
        polish::resolve_endpoint(&config.ai.effective_base_url()).map_err(|e| e.to_string())?;
    if !target.is_allowed {
        return Err(PolishError::EndpointNotAllowed.to_string());
    }
    if !target.is_hosted {
        return Err("This endpoint is on your machine; nothing is sent.".to_string());
    }

    // Record under the lock and re-read there, so a settings write landing
    // between the read above and this point is not silently overwritten with a
    // stale clone. On a failed disk write the in-memory list is put back, so
    // memory and disk never disagree about what was consented to.
    if !is_consented(&config.ai, &target.host) {
        let updated = {
            let mut guard = recover_poison(state.config.lock(), "commands::ai::ai_consent_host");
            guard.ai.consented_hosts.push(target.host.clone());
            guard.ai.consented_hosts.sort();
            guard.ai.consented_hosts.dedup();
            guard.clone()
        };
        if let Err(reason) = super::config::persist_config(&state, &updated) {
            let mut guard = recover_poison(state.config.lock(), "commands::ai::ai_consent_host");
            guard.ai.consented_hosts.retain(|h| h != &target.host);
            return Err(reason);
        }
        config = updated;
        announce_ai_config(&app);
    }

    // Reached only for a hosted endpoint, which does need a key.
    let key_state = key_state_for(&app, &config.ai.provider);
    Ok(endpoint_state_from(&config.ai, key_state))
}

/// The model a provider starts from: the row's own default when the list it
/// answered this session holds it, else the first of the row's suggestions
/// that list holds, else the one model a local runtime lists, and nothing
/// when no list has been read for it yet.
///
/// The rule lives in `writ_core::ai::models::seed_model`; this reads the
/// catalog held for the provider and applies it. Nothing is written that the
/// account's own list does not hold, so the list arriving is what writes the
/// first model ([`seed_from_catalog`]).
fn seed_model_for(app: &AppHandle, provider: &str) -> String {
    let held = app.state::<AiState>().live_catalog(provider);
    seed_model(provider, held.as_ref())
}

/// Writes the model a list the provider answered seeds, when the connection
/// has none.
///
/// First use of a provider: the row is chosen before anything is known about
/// what it serves, so the model is written when its own list arrives rather
/// than guessed from the table. A model already in the file is left alone. A
/// list that could not be read, or that holds none of the row's ids and is not
/// a local runtime's list of one, writes nothing and leaves the choice to the
/// picker. A list that names another provider is ignored, because the
/// connection may have moved while it was in flight.
fn seed_from_catalog(app: &AppHandle, catalog: &ModelCatalog) {
    let seed = seed_model(&catalog.provider, Some(catalog));
    if seed.is_empty() {
        return;
    }
    let state = app.state::<AppState>();
    let (updated, previous) = {
        let mut guard = recover_poison(state.config.lock(), "commands::ai::seed_from_catalog");
        if guard.ai.provider != catalog.provider || !guard.ai.model.trim().is_empty() {
            return;
        }
        let previous = guard.ai.model.clone();
        guard.ai.model = seed;
        (guard.clone(), previous)
    };
    if let Err(reason) = super::config::persist_config(&state, &updated) {
        let mut guard = recover_poison(state.config.lock(), "commands::ai::seed_from_catalog");
        guard.ai.model = previous;
        tracing::warn!(error = %reason, "the first model could not be saved");
        return;
    }
    announce_ai_config(app);
}

/// Tells the frontend the `[ai]` section changed.
///
/// `persist_config` records its own write in the watcher's ignore set, so the
/// file change never returns as external: without this the copy the frontend
/// holds keeps the value it read before, and its next settings write sends that
/// copy whole and undoes what was just written.
fn announce_ai_config(app: &AppHandle) {
    if let Err(error) = emit_event(
        app,
        WritFrontendEvent::ConfigChanged {
            keys: vec!["ai".to_string()],
        },
    ) {
        tracing::warn!(error = %error, "failed to emit config event");
    }
}

/// Points the connection at another provider and saves.
///
/// The one place a provider change happens. The rule that a chat model belongs
/// to the server it was picked from lives in [`AiConfig::with_provider`], so
/// the settings panel and the pane cannot clear an override differently.
#[tauri::command]
pub fn ai_set_provider(app: AppHandle, provider: String) -> Result<AiConfig, String> {
    if providers::provider(&provider).is_none() {
        return Err("That provider is not one this version knows.".to_string());
    }
    let state = app.state::<AppState>();
    let seed = seed_model_for(&app, &provider);

    // Read, change and clone under one lock, so a settings write landing
    // between a read and a write is not overwritten with a stale copy.
    let (updated, previous) = {
        let mut guard = recover_poison(state.config.lock(), "commands::ai::ai_set_provider");
        let previous = guard.ai.clone();
        guard.ai = previous.with_provider(&provider, &seed);
        (guard.clone(), previous)
    };
    if let Err(reason) = super::config::persist_config(&state, &updated) {
        let mut guard = recover_poison(state.config.lock(), "commands::ai::ai_set_provider");
        guard.ai = previous;
        return Err(reason);
    }
    Ok(updated.ai)
}

fn emit_ai(app: &AppHandle, request_id: &str, kind: &str, text: Option<String>) {
    if let Err(e) = emit_event(
        app,
        WritFrontendEvent::AiRewrite {
            request_id: request_id.to_string(),
            kind: kind.to_string(),
            text,
        },
    ) {
        tracing::warn!(error = %e, "failed to emit ai-rewrite event");
    }
}

/// Starts a streaming rewrite of `text`. The frontend supplies `request_id` so
/// it can match `writ://ai-rewrite` events (and cancel) with no window in which
/// an early event — e.g. an immediate connection-refused error — could arrive
/// unmatched. Validation runs synchronously; the network work is spawned.
#[tauri::command]
pub async fn ai_rewrite(
    app: AppHandle,
    request_id: String,
    action: String,
    text: String,
    custom_instruction: Option<String>,
) -> Result<String, String> {
    let cfg = {
        let state = app.state::<AppState>();
        let guard = recover_poison(state.config.lock(), "commands::ai::ai_rewrite");
        guard.ai.clone()
    };

    let prepared = prepare_request(&cfg, &action, &text, custom_instruction, |preset| {
        key_for(&app, preset)
    })
    .map_err(|e| e.to_string())?;

    tracing::info!(text_len = text.len(), "starting rewrite");

    let client = build_client()?;

    let cancel = CancellationToken::new();

    // Register the token before spawning so a cancel that races the task can
    // never miss it, and so the task's self-removal has an entry to remove.
    app.state::<AiState>()
        .track_rewrite(&request_id, cancel.clone());

    let task_app = app.clone();
    let task_id = request_id.clone();
    tauri::async_runtime::spawn(async move {
        run_rewrite_stream(&client, &prepared, &cancel, |event| match event {
            StreamEvent::Chunk(content) => emit_ai(&task_app, &task_id, "chunk", Some(content)),
            StreamEvent::Done => emit_ai(&task_app, &task_id, "done", None),
            StreamEvent::Error(message) => emit_ai(&task_app, &task_id, "error", Some(message)),
        })
        .await;

        task_app.state::<AiState>().finish_rewrite(&task_id);
    });

    Ok(request_id)
}

/// Stops an in-flight rewrite, one still waiting for the host's first byte
/// included. Nothing further is emitted; the frontend already discarded the
/// preview.
#[tauri::command]
pub fn ai_cancel(ai: State<'_, AiState>, request_id: String) {
    ai.cancel_rewrite(&request_id);
}

/// The provider table, so the settings dropdown, the probe and the model list
/// read one definition of a row rather than three.
#[tauri::command]
pub fn ai_providers() -> Vec<ProviderInfo> {
    providers::PROVIDERS.to_vec()
}

/// Reads the model list of the configured connection.
///
/// The list carries the key, so it is a send: a hosted host that has not been
/// allowed answers `ConsentRequired` and nothing leaves the machine
/// ([`connection::read_catalog`]).
#[tauri::command]
pub async fn ai_list_models(app: AppHandle) -> ModelCatalog {
    let cfg = {
        let state = app.state::<AppState>();
        let guard = recover_poison(state.config.lock(), "commands::ai::ai_list_models");
        guard.ai.clone()
    };

    let catalog = connection::read_catalog(&cfg, |provider| key_for(&app, provider)).await;
    app.state::<AiState>().remember_catalog(catalog.clone());
    seed_from_catalog(&app, &catalog);
    catalog
}

/// Reports which local runtimes are running, so the provider rows can say so
/// before one is chosen.
#[tauri::command]
pub async fn ai_probe_local() -> LocalProbe {
    connection::probe_local().await
}

/// Probes the configured endpoint's model list so the UI can show connection
/// state before a rewrite is attempted ([`connection::check_connection`]).
#[tauri::command]
pub async fn ai_check_connection(app: AppHandle) -> Result<AiConnectionStatus, String> {
    let cfg = {
        let state = app.state::<AppState>();
        let guard = recover_poison(state.config.lock(), "commands::ai::ai_check_connection");
        guard.ai.clone()
    };
    Ok(connection::check_connection(&cfg, |provider| key_for(&app, provider)).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_here_is_in_the_invoke_handler() {
        // A command that is not registered cannot be called however well it
        // behaves, and the settings section calls all of these on open.
        const LIB_RS: &str = include_str!("../lib.rs");
        for command in [
            "commands::ai::ai_providers",
            "commands::ai::ai_list_models",
            "commands::ai::ai_probe_local",
            "commands::ai::ai_check_connection",
            "commands::ai::ai_endpoint_state",
            "commands::ai::ai_consent_host",
            "commands::ai::ai_set_api_key",
            "commands::ai::ai_clear_api_key",
            "commands::ai::ai_has_api_key",
        ] {
            assert!(LIB_RS.contains(command), "{command} is not registered");
        }
    }

    #[test]
    fn the_table_is_answered_whole() {
        let rows = ai_providers();
        assert_eq!(rows.len(), writ_core::ai::providers::PROVIDERS.len());
        assert_eq!(rows[0].id, "ollama");
        let json = serde_json::to_string(&rows).expect("serialised");
        assert!(json.contains("\"group\":\"local\""), "{json}");
        assert!(json.contains("\"wire\":\"anthropic\""), "{json}");
    }
}
