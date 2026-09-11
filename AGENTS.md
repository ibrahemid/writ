# Writ

Lightweight, always-ready text editor for developers. Tauri v2 + SolidJS + CodeMirror 6.

## Commands

```bash
cargo fmt --all --check                       # Formatting (CI enforces this)
cargo test --workspace                        # 50+ Rust tests
cargo clippy --workspace -- -D warnings       # Zero warnings policy
cargo tauri dev                               # Run the app
npx tsc --noEmit                              # TypeScript check
pnpm build                                    # Frontend build
```

## Architecture Rules (ENFORCED — do not violate)

### Rust: Cargo Workspace Boundaries
- `crates/writ-core/` — Pure Rust. Zero Tauri dependency. If it imports tauri, it won't compile.
- `crates/writ-storage/` — SQLite, file I/O. Depends on writ-core only.
- `crates/writ-plugin/` — Plugin types. Depends on writ-core only.
- `src-tauri/` — ONLY crate that imports tauri. Thin adapter.

### Frontend: Import Rules
- `src/services/tauri.ts` is the ONLY file that imports `@tauri-apps/api`. No exceptions. No dynamic imports of Tauri in components.
- `src/services/events.ts` is the ONLY file that imports `@tauri-apps/api/event`.
- Components call stores. Stores call services. Services call Tauri. Never skip layers.

### Frontend: Component Rules
- No `document.querySelector` in components or stores. Use refs or store-managed state.
- No `document.addEventListener` outside `onMount`/`createEffect`. Always clean up in `onCleanup`.
- Module-level signals are acceptable ONLY as singleton patterns (single-window app). Mark with comment: `// Singleton state — Writ is single-window`.

### Rust: Module Rules
- `commands/` is the IPC surface. It calls state/storage, never the reverse.
- Core decides policy, Tauri executes mechanism (e.g., watcher conflict resolution).
- Watcher uses IgnoreSet to skip internal writes. Always insert filename before writing.

## Quality Gates (run before claiming work is done)

1. `cargo fmt --all --check` — no diffs (CI fails on this)
2. `cargo test --workspace` — all pass
3. `cargo clippy --workspace -- -D warnings` — zero warnings
4. `npx tsc --noEmit` — zero errors
5. `pnpm build` — builds clean
6. If you changed frontend behavior, verify in the running app

## Testing Requirements

- New Rust functions MUST have tests
- New IPC commands MUST have corresponding test coverage
- Do NOT skip tests to save time. If tests are hard to write, that's a design smell.

## Design Spec

See `docs/ARCHITECTURE.md` for the system design and `docs/adr/` for architecture decision records.
Read these before making architectural decisions.

## Context Budget (ENFORCED)

- Run gates through `scripts/gate.sh [fmt|test|clippy|tsc|build|vitest]`. It prints one line per check and failure detail only on failure; full logs in `.status/gate-logs/`. Raw `cargo test`/`clippy`/`tsc` only when the gate output is insufficient.
- Agent prompts pass file paths, never file contents. Do not `cat` a file to hand it to an agent.
- Read in ranges (`sed -n`, `grep -n`, Read with offset/limit). Whole-file dumps only under 150 lines.
- Subagent final reports: 180 words max, template in the agent definitions, detail in `.status/reports/`.
- Orchestration sessions end at the unit boundary. Start the next unit in a fresh session from `.status/v2/PROGRESS.md`. Never resume a session after a gap over an hour.

## Git

- Commit messages: type(scope): description
- Types: feat, fix, refactor, docs, test, chore
- Trunk-based: main is the only long-lived branch; short-lived branches PR into main
- Do NOT commit to main directly — use branches; merges need operator authorization
- Do NOT add Co-Authored-By lines

### Message style (public-facing)
Commit subjects/bodies and PR titles/descriptions are read by strangers. Natural and trimmed: what changed, only what's worth knowing.
- Subject: `type(scope): description`, lowercase, concise.
- Body: short. Load-bearing facts only (real file/crate/module names, a bare ADR number if it's the record). For a big change, gist plus a couple of concrete items, not everything ("…and some fixes" is fine).
- NEVER: codenames/design labels (vfinal, v3, phase N, part N/N, L1-L6); process words (blocker, launch-blocker, audit, surgical, philosophy, re-scope); session/agent references; effort narration; verbose `Decision record: docs/adr/…` lines; the word "marketing" (say "site"/"website").
- No em dashes, no first person, no closing chrome.

## Key Shortcuts (current)

| Shortcut | Action |
|---|---|
| Cmd+Shift+Space | Toggle window (global; the shortcut editor rebinds it when another app holds it) |
| Cmd+N | New note |
| Cmd+T | New note (alias) |
| File menu | Today's note |
| Cmd+Shift+O | Open note by name |
| Cmd+W | Close tab |
| Cmd+[ / Cmd+] | Switch tabs |
| Cmd+S | Save |
| Cmd+, | Settings |
| Cmd+\ (alias Cmd+Option+S) | Toggle sidebar |
| Cmd+Shift+\ | Toggle connections panel |
| Cmd+Shift+H | Swap preview split orientation |
| Cmd+F | Find in document |
| Cmd+Option+F | Replace (also the Find bar's own control) |
| Shift+Shift | Command palette |
| Double-click tab | Rename tab |
| Cmd+D | Duplicate line/selection |
| Cmd+Shift+K | Delete line |
| Shift+Alt+Up/Down | Move line |
| Cmd+/ | Toggle comment |
| Cmd+L | Select line |
| Cmd+Enter / Cmd+Shift+Enter | Insert line below/above |
| Cmd+Shift+J | Join lines |
| Cmd+Shift+D | Select next occurrence |
| Cmd+Shift+E | Toggle inline code (markdown) |

## Data Flow

Hotkey → Tauri → Rust → SQLite/disk
Frontend → stores → services/tauri.ts → IPC → Rust commands
Rust events → Tauri emit → services/events.ts → stores → components

## Scoped rules

For frontend TypeScript/SolidJS changes, read `.claude/rules/frontend.md`. For Rust changes, read `.claude/rules/rust.md`. These project-owned rules apply to both agents.
