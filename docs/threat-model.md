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
| The MCP path makes no outbound request. Every read tool answers with the network denied. | Satisfied by `the_resolved_tree_carries_no_http_client`, `crates/writ-mcp/tests/no_tauri_dependency.rs`, which walks `cargo metadata` from `writ-mcp` over normal edges and fails on `tauri`, `reqwest`, `hyper`, `ureq` or `wry`; `rmcp` is taken with `default-features = false` so the HTTP transports resolve out. Still walked with the network-denied run | Shipped |
| Every write to a note file goes through the guard, and a refusal leaves the losing side on disk as a conflict copy. | `writ_core::notes::guard::decide_save:65` has one caller after U3; `write_conflict_copy`, `crates/writ-storage/src/buffer_store.rs:1057`; today three copies exist at `buffer_store.rs:638`, `note_ops.rs:187`, `note_ops.rs:357` | U3 |
| A client that has not been approved is refused, and its call performs no read and no write. | `writ_mcp::consent::ConsentGate` with the `DenyAll` default, `crates/writ-mcp/src/consent.rs`, refusing every tool asserted by `with_the_server_off_every_tool_is_refused`; the gate against `[mcp] approved_clients` (U5) | U4, U5 |
| No tool deletes or trashes a note. The registered write tools are `write_note`, `create_note` and `rename_note`. | Tool list asserted in `crates/writ-mcp` tests; `writ trash` (`crates/writ-cli/src/verbs.rs:70`) is the user's own CLI, not a client | U6 |
| Consent is per client and split into read and write. Approving reading never grants writing. | `ClientApproval { name, first_seen, read, write }` in `[mcp] approved_clients` | U5 |
| A call from an unknown client appends exactly one pending record, so the user can see it and decide. | `writ_storage::activity_log::append` with `Decision::Pending` | U5 |
| Consent is revocable in the app and by editing `config.toml`, and takes effect on the next call without a restart. | `ai.consented_hosts` (`crates/writ-core/src/config/ai.rs:56`) with `ai_consent_host` (`src-tauri/src/commands/ai.rs:355`) for hosts; `mcp_set_client_permission` and `mcp_forget_client` for clients, against an mtime-guarded gate | U5 |
| A fresh configuration reaches no network and answers no client. `ai.enabled`, `mcp.enabled` and `ai.chat.enabled` are all false. | `crates/writ-core/src/config/ai.rs:12` for `ai.enabled`; `McpConfig::enabled`, `crates/writ-core/src/config/mcp.rs`; `[ai.chat] enabled` (U7) | U4, U7 |
| The server is a stdio process the client launches. Writ listens on no port. | `writ mcp`, `crates/writ-cli/src/mcp.rs`, served over stdio by `writ_mcp::server::serve_stdio`; checked with `lsof` against the running server | U4 |
| The activity log is append-only and capped. It rotates at 5 MB and keeps one generation, and a malformed line is skipped. | `crates/writ-storage/src/activity_log.rs`, `O_APPEND` writes at `<data dir>/activity.jsonl` | U5 |

## Open gap

The third row is now testable at the dependency level, which is where it was open. What the test
cannot see is a request made through a crate it does not recognise, so the network-denied run at
the end of the release still happens: the test says no HTTP client is reachable, the run says no
request was made.
