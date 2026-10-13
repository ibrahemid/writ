use crate::state::AppState;

/// How often the heartbeat thread `lib.rs` runs beats.
pub const SNAPSHOT_HEARTBEAT: std::time::Duration = std::time::Duration::from_secs(120);

/// One beat: records that this session is running, if this process has not
/// yet (ADR-044 §6).
///
/// The marker carries no text and the beat reads no file. A beat used to read
/// every open tab's file into a snapshot while holding the store, and a tab on
/// a network share that stopped answering held it for as long as the read
/// blocked, which held every save and the main thread behind it. The text a
/// crash would lose is the journal's now, written as each save that could not
/// land returns, so there is nothing left for a beat to read.
///
/// Every beat takes the store, which keeps the flag that says the marker is
/// written. The first beat of a session writes it, one insert into `writ.db`;
/// every beat after that reads the flag and does no I/O.
pub fn beat(state: &AppState) {
    let marked = state
        .store
        .lock()
        .ok()
        .map(|mut store| store.mark_session_running());
    if let Some(Err(e)) = marked {
        tracing::warn!(error = %e, "the running-session marker could not be written");
    }
}
