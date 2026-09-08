# Threat model checklist

The assertions [ADR-031](./adr/031-the-ai-harness-and-what-leaves-the-machine.md) is walked
against. Each one is checked against shipped code at the end of the 0.5 release, in this order.

`Shipped` means the code that satisfies the assertion is in the tree today and the evidence column
names it. A unit id means the assertion is satisfied by that unit of the 0.5 release, and the row
is not walkable until it merges.

| Assertion | Evidence | Status |
|---|---|---|
| No API key reaches `config.toml`, the database or disk. Keys are in the OS keychain, or in memory for the session where there is no native keychain. | `src-tauri/src/commands/ai.rs:81` (`mod keychain`, service at `:86`), stub at `:115`, session fallback `AiState.keys:50`, invariant stated at `:16-20` | Shipped |
| No note content, prompt text or response text reaches a log, a database row or an error string shown to the user. | `src-tauri/src/commands/ai.rs:21-22` for the rewrite path; `sanitize_update_error`, `src-tauri/src/commands/update.rs:293`, for the updater; `writ_core::activity::ActivityRecord` has no content field | U5 |
| The MCP path makes no outbound request. Every read tool answers with the network denied. | `crates/writ-mcp/Cargo.toml` carries no HTTP client, asserted by `crates/writ-mcp/tests/no_tauri_dependency.rs` for tauri only; walked with the network-denied run | U4 |
| Every write to a note file goes through the guard, and a refusal leaves the losing side on disk as a conflict copy. | `writ_core::notes::guard::decide_save:65` has one caller after U3; `write_conflict_copy`, `crates/writ-storage/src/buffer_store.rs:1057`; today three copies exist at `buffer_store.rs:638`, `note_ops.rs:187`, `note_ops.rs:357` | U3 |
| A client that has not been approved is refused, and its call performs no read and no write. | `writ_mcp::consent::ConsentGate` with the `DenyAll` default (U4), `ConfigGate` against `[mcp] approved_clients` (U5) | U4, U5 |
| No tool deletes or trashes a note. The registered write tools are `write_note`, `create_note` and `rename_note`. | Tool list asserted in `crates/writ-mcp` tests; `writ trash` (`crates/writ-cli/src/verbs.rs:70`) is the user's own CLI, not a client | U6 |
| Consent is per client and split into read and write. Approving reading never grants writing. | `ClientApproval { name, first_seen, read, write }` in `[mcp] approved_clients` | U5 |
| A call from an unknown client appends exactly one pending record, so the user can see it and decide. | `writ_storage::activity_log::append` with `Decision::Pending` | U5 |
| Consent is revocable in the app and by editing `config.toml`, and takes effect on the next call without a restart. | `ai.consented_hosts` (`crates/writ-core/src/config/ai.rs:56`) with `ai_consent_host` (`src-tauri/src/commands/ai.rs:355`) for hosts; `mcp_set_client_permission` and `mcp_forget_client` for clients, against an mtime-guarded gate | U5 |
| A fresh configuration reaches no network and answers no client. `ai.enabled`, `mcp.enabled` and `ai.chat.enabled` are all false. | `crates/writ-core/src/config/ai.rs:12` for `ai.enabled`; `[mcp] enabled` (U4); `[ai.chat] enabled` (U7) | U4, U7 |
| The server is a stdio process the client launches. Writ listens on no port. | `writ mcp` served over stdio from `crates/writ-cli` (`main.rs:138` for path resolution); checked with `lsof` against the running server | U4 |
| The activity log is append-only and capped. It rotates at 5 MB and keeps one generation, and a malformed line is skipped. | `crates/writ-storage/src/activity_log.rs`, `O_APPEND` writes at `<data dir>/activity.jsonl` | U5 |

## Open gap

The third row has no automated check behind it. U4 asserts that `writ-mcp` has no `tauri`
dependency and nothing more, so "the MCP path makes no outbound request" rests on the manual
network-denied run at the end of the release. A dependency-level assertion over the crate's
resolved tree would make the row testable.
