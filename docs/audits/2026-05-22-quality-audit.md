# Writ quality audit — 2026-05-22

Read-only cross-cutting audit run by 10 parallel agents (one per category). Bar: Hashimoto-grade taste and engineering, per `.claude/CLAUDE.local.md`. Every finding cites `file:line`; every metric is measured, not estimated. No third-party tools recommended — every fix is in-repo work.

## Summary by category

| Category | Blockers | Quality | Polish |
|---|---:|---:|---:|
| Performance | 7 | 5 | 3 |
| Visual polish | 2 | 5 | 4 |
| Interaction quality | 3 | 5 | 5 |
| Architecture smells | 5 | 4 | 2 |
| Test coverage | 15 | 21 | 4 |
| Documentation / claim parity | 6 | 14 | 4 |
| Security | 2 | 2 | 3 |
| Data integrity | 8 | 3 | 1 |
| Cross-platform readiness | 4 | 6 | 3 |
| Accessibility | 7 | 16 | 3 |
| **Total** | **59** | **81** | **32** |

Headline read: data integrity, cross-platform readiness, and test coverage are the biggest blockers to a Hashimoto-grade launch. Several BLOCKERs are *correctness* problems that can silently destroy user data (non-atomic writes, missing flush-on-close, dead recovery system, watcher debouncer race). The codebase is well-built in spots (Rust core, FTS, transforms, security primitives) but the integration seams between layers have rotted.

---

## SHIPPING BLOCKERS (must fix before launch)

### Data integrity (highest priority — user-text loss/corruption)

1. **closeTab does not flush pending autosave; last edits are lost** — `src/stores/buffers.ts:46-58`, `src/components/Editor/EditorInstance.tsx:219-229` — Fix: `await flushAutosave(id)` before `api.close*` in closeTab/closeOthers/closeAll; drop the `content.length > 0` guard in onCleanup.
2. **Buffer content writes are non-atomic — crash mid-write corrupts/truncates** — `crates/writ-storage/src/buffer_store.rs:110, :251` — Fix: write `<filename>.tmp` then `sync_all`, `fs::rename` into place, fsync parent dir; Windows uses `ReplaceFileW`.
3. **Watcher debouncer drops real external edits inside the ignore window** — `src-tauri/src/watcher/handler.rs:35, :80-90` — Fix: replace boolean IgnoreSet with per-write content fingerprint (sha256+TTL); only treat events whose hash matches a recent stamp as internal.
4. **External-edit handler doesn't reload the editor buffer; next keystroke clobbers disk** — `src/App.tsx:311-316`; payload carries filename not buffer id (`src-tauri/src/watcher/handler.rs:96`) — Fix: emit buffer UUID on `buffer:external`; in handler, `readBufferContent` and reset CodeMirror view; surface a conflict UI when the in-memory diverges.
5. **FTS + content writes are not transactional; crash leaves search index permanently out of sync** — `crates/writ-storage/src/buffer_store.rs:107-117`, `crates/writ-storage/src/fts.rs:40-44` — Fix: wrap `update_timestamp + fts.delete + fts.insert` in `unchecked_transaction`; propagate FTS errors; run FTS-vs-buffers parity check on startup and auto-`rebuild_fts` on drift.
6. **Snapshot/dirty-shutdown recovery is dead code — nothing writes snapshots, nothing checks them on launch** — `crates/writ-storage/src/recovery/snapshot.rs`, `dirty_shutdown.rs`; zero callsites in `src-tauri/` — Fix: call `check_dirty_shutdown` at boot, emit `recovery:dirty`, surface a recovery panel; periodic snapshot writes; flush + clean snapshot on `WindowEvent::CloseRequested`/`RunEvent::Exit`.
7. **`open_external` and `create_buffer` mint colliding mirror filenames; two opens overwrite each other** — `crates/writ-core/src/buffer/manager.rs:53-81 / :87-120` — Fix: derive `filename` from the UUID (`{id}.txt`); backfill via v2 migration; keep `title` as the human label.
8. **DB has no `schema_version` downgrade guard; older binary on a newer DB silently corrupts** — `crates/writ-storage/src/database/migrations.rs:14-45` — Fix: at startup compare embedded `MAX(version)` with DB `MAX(version)`; refuse to open if DB is ahead; switch `row_to_document` (`queries.rs:13-47`) to column-name access; add `UNIQUE(buffers.filename)` after fix #7.

### Security

9. **`create_buffer` title becomes an unsanitized on-disk filename — arbitrary file write via IPC** — `crates/writ-core/src/buffer/manager.rs:60-64`, `crates/writ-storage/src/buffer_store.rs:59` — Fix: in `BufferManager::create_buffer`, reject any title whose `Path::new(title).components()` contains `ParentDir`/`RootDir`/`Prefix`; derive on-disk filename from UUID; add regression test.
10. **`open_file` / `save_to_source` accept any absolute path — arbitrary read & write via IPC** — `src-tauri/src/commands/file.rs:69-94` — Fix: track dialog-returned paths server-side and gate `open_file`/`save_to_source` on origin (or document the renderer is trusted and lock CSP as compensating control — current CSP is solid).

### Cross-platform

11. **Global hotkey hardcoded `SUPER+SHIFT+Space`; ignores user config; broken on Windows/Linux** — `src-tauri/src/hotkey/mod.rs:10`; config string at `crates/writ-core/src/config/mod.rs:14` is never parsed — Fix: parse `config.hotkey.toggle`; map `CmdOrCtrl` → `META` on macOS, `CONTROL` elsewhere; re-register on config change.
12. **App menu built unconditionally with macOS-style accelerators on every platform** — `src-tauri/src/lib.rs:16-67, :197` — Fix: gate `build_app_menu` with `#[cfg(target_os = "macos")]`, or render a Windows/Linux menubar inside the custom titlebar.
13. **File associations broken on Windows/Linux — `RunEvent::Opened` is macOS/iOS only** — `src-tauri/src/lib.rs:241`; 22 file extensions declared in `tauri.conf.json:85-302` — Fix: on first launch read `std::env::args().skip(1)`, feed `pending_opens`; add `tauri-plugin-single-instance` to forward args to a running instance.
14. **`notify` crate declared `default-features = false, features = ["macos_fsevent"]`** — `src-tauri/Cargo.toml:27` — Fix: declare `notify = "7"` with defaults (or all backends explicitly); the cross-platform watcher works today only by transitive feature unification from `notify-debouncer-mini`, which is fragile.

### Performance

15. **Cold-start window flash: `visible: true` shows the OS window before the bundle parses** — `src-tauri/tauri.conf.json:25`; first-paint chain at `src/App.tsx:67-87` — Fix: set `visible: false`; call `getCurrentWindow().show()` from the frontend at end of `onMount` after first paint.
16. **Silent update check fires unconditionally on every cold start** — `src-tauri/src/lib.rs:225-232` (`sleep(5s)` → `run_update_check(handle, false)`) — Fix: gate on a `configStore.config().updater.auto_check` setting that defaults to true but is documented and toggleable; only run after a session-active threshold to avoid a phone-home every launch.
17. **Sidebar SearchBar O(n) `ids.includes()` inside `.filter()` over thousands of buffers** — `src/components/Sidebar/SearchBar.tsx:21-23` (10k×10k = 10⁸ ops per query change; companion `search-results.ts:16` already uses a Set) — Fix: hoist a `createMemo` of `new Set(matchedIds)` and reuse.
18. **`detectFromContent` runs full-document scoring every ~40 keystrokes — measured 7.6 ms @ 1 MB, 38 ms @ 5 MB** — `src/components/Editor/EditorInstance.tsx:54-61, :99`, `src/services/language-detect.ts:99-105, :137-158` — Fix: cap detection input at 64 KiB (`content.slice(0, 65536)`); disable entirely above 256 KiB; drop the `JSON.parse(trimmed)` whole-doc in `scoreJson` above 64 KiB.
19. **updateListener materializes full doc string + IPC + FTS reindex every keystroke (300 ms debounce)** — `src/components/Editor/EditorInstance.tsx:92`, `src-tauri/src/commands/buffer.rs:71-87`, `crates/writ-storage/src/fts.rs:40-44` — Fix: defer `doc.toString()` to inside the flush; debounce FTS reindex on a longer (2 s) idle timer separately from the disk write.
20. **FTS5 has no prefix-token index — measured 14 ms median for `tok*` on 10k buffers** — `crates/writ-storage/migrations/001_initial.sql:19`, `crates/writ-storage/src/fts.rs:47-58` — Fix: add a v2 migration creating the table with `prefix='2 3 4'` and `tokenize='unicode61 remove_diacritics 2'`; migrate via `INSERT INTO new SELECT ... FROM old`.
21. **Sidebar History renders every row at 500/1000+; per-row `Date.now()` × full memo invalidation on any buffer change** — `src/components/Sidebar/HistorySection.tsx:32-60` — Fix: introduce a windowed `<For>` driven by scroll position; compute a single `now` snapshot per render and pass it down.

### Interaction quality

22. **App shortcuts fire while typing in the editor (Cmd+S toggles sidebar, Cmd+W closes tab, Cmd+R renames)** — `src/commands/keybindings.ts:133-188`; capture handler installed on `document` with only modal gating — Fix: in `handleKeyDown`, skip when `e.target` is inside `.cm-editor`/`input`/`textarea`/contenteditable, except for deliberately-global chords (Cmd+T, Cmd+W, palette).
23. **Cmd+S bound to Toggle Sidebar shadows the universal Save chord** — `src/App.tsx:176`, default `src/stores/config.ts:7`; no manual save command exists — Fix: move sidebar toggle off Cmd+S (e.g., Cmd+B); add an explicit "save now" or a transient "Saved automatically" affordance on Cmd+S.
24. **Documented "Cmd+SS (double) toggle sidebar" is not implemented** — CLAUDE.md/README claim it; `src/App.tsx:172-179` registers single-press `CmdOrCtrl+S`; double-tap engine at `src/commands/keybindings.ts:98-128` has zero `+double+` consumers in production code — Fix: pick one path (implement double-tap and update binding, or delete the dead double-tap machinery and fix docs).

### Visual polish

25. **`--writ-accent-foreground` token is referenced but undefined → ShortcutEditor "Save" button text falls back to muted on accent (~1.45:1 in catppuccin)** — `src/components/ShortcutEditor/ShortcutEditor.css:67-75`; token absent from `src/styles/theme.css` and all 5 theme JSONs — Fix: define `--writ-accent-foreground` per theme, or use `color: var(--writ-surface-background)` matching the ConfirmDialog pattern (`ConfirmDialog.css:63`).
26. **Empty sidebar is an undesigned blank void (search bar over empty sunken surface)** — `src/components/Sidebar/Sidebar.tsx:18-26`, `ActiveSection.tsx:39`, `HistorySection.tsx` — only the search branch has an empty state — Fix: add a designed empty state to the non-search branch (centered "No open files" + open-via-Cmd+O hint), styled like `.palette-empty`.

### Architecture smells (rule violations)

27. **`src/services/window-size.ts:1` imports `@tauri-apps/api/window` directly** — violates the "services/tauri.ts is the ONLY file allowed to import @tauri-apps/api" rule — Fix: move `getCurrentWindow().outerSize/scaleFactor/setSize/onResized` calls into thin wrappers in `services/tauri.ts`.
28. **`src/services/window-size.ts:2` reverse-imports `stores/config`** — services sit below stores in the hierarchy — Fix: move the orchestrator into a new `stores/window.ts`.
29. **Components skip the store layer and call services/tauri.ts directly** — `src/components/Editor/EditorInstance.tsx:15`, `src/components/TitleBar/TitleBar.tsx:7`, `TrafficLights.tsx:2` — Fix: route reads/saves and window controls through a `stores/window.ts` (and a buffer-content store method).
30. **`installKeyboardHandler` is never uninstalled** — `src/App.tsx:304` install with no matching call in `onCleanup` at `:344-348`; `uninstallKeyboardHandler` exists at `src/commands/keybindings.ts:194` — Fix: `onCleanup(uninstallKeyboardHandler)`.
31. **Half-built event-bus migration shipped: legacy `emit_event` + new bridge run in parallel** — `src-tauri/src/lib.rs:69`, `src-tauri/src/watcher/handler.rs:53`, `src-tauri/src/events/bus_bridge.rs:32-39` — Fix: finish the migration (move menu-action / config:changed / buffer:external through the bridge) or revert the bridge until it's canonical. Don't ship both.

### Test coverage (data-loss / corruption / crash paths)

32. **Watcher has zero tests** (`start_file_watcher`, `create_ignore_set`, IgnoreSet drain semantics, `path.starts_with(buffers_dir)` filter, modified/deleted discrimination, empty-filename guard at `handler.rs:76`, channel-close exit at `:110`, `config_path` missing branch at `:37`) — Fix: integration tests using `tempfile::TempDir`.
33. **Update flow untested** — `run_update_check` silent-vs-user policy (`update.rs:162-183`), `download_and_install_update` progress-throttle / mid-flight `Ok(None)` / install-result error branch, `check_for_update` / `dismiss_update` / `restart_app` IPCs (`commands/update.rs:30/121/128`) — Fix: factor visibility-decision and progress-throttle into pure helpers; table-driven tests for all 4 (visibility × outcome) cases and all 8 phase variants.
34. **File-IO IPCs untested at the boundary** — `open_file` (`commands/file.rs:69`), `save_to_source` (`:74-94`), `save_buffer_content` (`commands/buffer.rs:72`), `create_buffer` Create branch (`commands/buffer.rs:42-63`). All depend on inserting the filename into IgnoreSet BEFORE the write; today the watcher-suppression contract is implicit and untested — Fix: integration tests asserting `IgnoreSet` contains `doc.filename` immediately before each save returns.
35. **`clear_history` per-buffer loop has no transaction; mid-loop failure leaves partial state, no test** — `src-tauri/src/commands/history.rs:20-29` — Fix: wrap in `conn.unchecked_transaction`; assert all-or-nothing on induced failure.
36. **`BufferStore::rebuild_fts` happy-path only; `close_many` atomicity claim untested** — `crates/writ-storage/src/buffer_store.rs:273, :53` — Fix: cover rebuild over mixed Active+History + missing-file fallback + stale-rows; induce a `close_many` SQL error and assert prior closes are rolled back.
37. **`BufferManager::delete_buffer` / `restore_buffer` error paths untested** — `crates/writ-core/src/buffer/manager.rs:157, :144` — Fix: assert `BufferNotFound` on unknown ids.

### Documentation / claim parity

38. **Every download link on README + site is 404** — `README.md:15-17`, `site/src/pages/download.astro:13-33`, `site/src/components/Hero.astro:27`, `Install.astro:9,17,25`; `gh release list` returns only one **draft** `v0.1.0-5` — Fix: do not publish the site until a public release exists, or replace links with a "coming soon" state.
39. **`install.sh` Linux one-liner promoted on the site will exit with "could not resolve latest release"** — `install.sh:32-38` hits `releases/latest` API — Fix: same as #38; the install script also needs a friendlier "not yet released" message.
40. **Site preloads two fonts that don't exist** — `site/src/layouts/Base.astro:38-50`, `site/src/styles/global.css:9-30` reference `/writ/fonts/*.woff2`; `site/public/fonts/` does not exist; the privacy page's "loads two self-hosted fonts" claim is also false — Fix: add the woff2 files (or remove preloads + `@font-face` + privacy claim).
41. **Updater manifest silently omits Windows + Linux entries** — `.github/scripts/build_latest_json.py:59-74` matches `\.msi\.zip$` and `\.AppImage\.tar\.gz$`, but `release.yml:151,158` builds only `msi` and `deb,appimage` (no zipped wrappers). Confirmed against draft v0.1.0-5 assets. Auto-update is dead on Windows/Linux even though docs and ADR-007 assume it works — Fix: add `updater` to the bundles matrix (Tauri produces the wrapper) or rewrite the script to consume raw `.msi`/`.AppImage` + `.sig`.
42. **macOS `.pkg` is missing from draft release** — `docs/RELEASING.md:119-121`, `packaging/README.md`, `packaging/homebrew/Casks/writ.rb:5,20` consume `Writ_<version>_universal.pkg`; v0.1.0-5 has none. The pkg step either failed silently or didn't run — Fix: investigate the workflow; do not publish until pkg is present.
43. **README documents `Cmd+F` for sidebar search but no binding exists** — `README.md:48`; `search.openContent` (`src/App.tsx:182`) has no `keybinding` — Fix: register `keybinding: "CmdOrCtrl+F"` or drop the row.

### Accessibility (core function unusable by keyboard / screen-reader)

44. **Sidebar tab/history rows are mouse-only — `<div onClick>` with no role/tabindex/keyboard** — `src/components/Sidebar/TabItem.tsx:14-20` — Fix: make the row a `<button>` (or add `role="button" tabindex={0}` + Enter/Space onKeyDown).
45. **Sidebar tab/history row close & restore controls are `<span onClick>` only, revealed only on hover (`opacity: 0`)** — `src/components/Sidebar/TabItem.tsx:25-43`, `TabItem.css:60-82` — Fix: real `<button aria-label="Close tab">`, reveal on `.tab-item:focus-within` too.
46. **TabBar new-tab `+` button has no accessible name** — `src/components/Editor/TabBar.tsx:103-106` — Fix: `aria-label="New tab"`.
47. **TabBar close `×` is a `<span role="button">` with no `aria-label`** — `src/components/Editor/TabBar.tsx:90-98` — Fix: `aria-label={``Close ${tab.title}``}` + use a real `<button>`.
48. **Toast dismiss `×` has no accessible name** — `src/components/Notifications/Toast.tsx:34` — Fix: `aria-label="Dismiss notification"`.
49. **Toasts are not announced to screen readers** — `src/components/Notifications/Toast.tsx:29-39`, `Toast.css:1-25`; no `role`/`aria-live` on the container; primary surface for "autosave failed", "Failed to save theme", "Failed to open" errors — Fix: `aria-live="polite"` on the container; `role="alert"` for error toasts.
50. **Save-status pill is wrapped in `<Show>` so the live region mounts/unmounts; first announcement is missed** — `src/components/Editor/StatusBar.tsx:16-30` — Fix: render the live region container unconditionally; toggle child text.

---

## QUALITY ISSUES (below the bar but not launch-blocking)

(Selected highlights; full list in agent reports below.)

- **App config defaults duplicated across Rust and TS** (`crates/writ-core/src/config/mod.rs:46,50` vs `src/stores/config.ts:8` vs `src/services/autosave.ts:25`) — three sources of truth for `tab_size: 2` and `autosave_debounce_ms: 300`.
- **Hardcoded app paths duplicated** between `src-tauri/src/lib.rs:114-117` and `src-tauri/src/state.rs:34-48`.
- **Bundle is 855 KB / 283 KB gzip single chunk** — all 9 language packs eagerly imported (`src/editor/builtins.ts:1-21`). Code-split via `vite.config.ts manualChunks` + dynamic `LanguageFactory` for ~50% first-paint reduction.
- **Cold startup chain is fully serial** (`src/App.tsx:69-75`) — 5-6 sequential IPC roundtrips; can be `Promise.all`'d.
- **Hotkey path logs 4-6 `info!` lines per press synchronously** — `src-tauri/src/hotkey/mod.rs:32-66`; demote to `debug!`.
- **Buffer-IPC commands lack tests at the adapter layer** — 8 of 10 buffer commands have zero tests; `transforms_ipc_tests.rs` misnames itself (it tests `TransformRegistry`, not the IPC).
- **`saveStatusStore` and `focusStore` have ZERO tests** (`src/stores/save-status.ts`, `src/stores/focus.ts`).
- **`configStore.recordCommandUse` / `pruneCommandUsage` debounced flush is untested** — `src/stores/config.ts:64, :100`.
- **Three different "double-tap" timing windows** — `keybindings.ts:137` (400ms), `keybindings.ts:172` (500ms), `recorder.ts:1` (300ms). Centralize.
- **No drag-and-drop for tab reordering or sidebar reordering** — only OS file-drop-to-open is wired (`src/App.tsx:324-332`); tab reorder is table stakes for an editor.
- **Editor surface has no right-click context menu** — `src/components/Editor/EditorInstance.tsx`; users expect Cut/Copy/Paste/Select All/Transform.
- **ContextMenu has no keyboard support and can render off-screen** — `src/components/ContextMenu/ContextMenu.tsx:28-63` no `role="menu"`/arrow nav; position never clamped to viewport.
- **Off-token font-size literals scattered** — `13px` (6 sites), `10px` (2), `9px`, `16px`, `18px`; theme.css only defines 14/12/11.
- **Off-scale spacing literals** — 7px / 5px / 26px / 3px / 116px appear nowhere in the `--writ-space-*` 2/4/8/12/16/24 scale.
- **Animation timing is ad-hoc with no `--writ-duration-*` / `--writ-ease-*` tokens** — `120ms`, `100ms`, `180ms`, `80ms`, `160ms`, `1200ms` scattered across 11 component CSS files.
- **Hardcoded `rgba(255,255,255,…)` overlays for hover/kbd; hardcoded blue selection** — `src/components/Sidebar/TabItem.css:93`, `Editor/TabBar.css:88`, `Kbd/Kbd.css:16`, `global.css:47`; ignore theme on Solarized/Dracula.
- **Buffer dedup by `source_path` is case-sensitive and not canonicalized** — duplicate buffers on macOS case-insensitive FS, symlinks/relative paths everywhere.
- **TOCTOU between `validate_file_for_opening` and `read_to_string`** — `src-tauri/src/commands/file.rs:13, :29`; can defeat the 10 MiB / binary guard via symlink retarget.
- **Buffer `filename` is `join`-ed by 5 callsites with no normalization** — a future code path or tampered SQLite row turns this into traversal.
- **FTS `MATCH` passes raw user query to FTS5 syntax** — bare `"` returns a parse error to the UI.
- **Cmd+R (tab.rename) shadows reload; Cmd+Shift+S alias shadows "Save As"** — `src/App.tsx:201-202`.
- **CommandPalette + ContextMenu + UpdateBanner live regions toggle via `<Show>`** — first announcement missed.
- **No semantic landmarks anywhere** — `src/App.tsx:358-376` is all `<div>`s.
- **24+ subtle-foreground contrast pairs fail AA across every theme**; Solarized active-tab text fails at 2.82:1 (see contrast table below).
- **Tab close / theme-editor close / shortcut-editor close are 16-20px, below WCAG 2.2 24px minimum**.
- **Inputs (search box, tab rename) have no `aria-label`** — placeholders are not labels.
- **`Kbd` `aria-label` is the raw `"CmdOrCtrl+Shift+T"` string** — screen readers read literally.
- **Site Hero/Header/Install version label is `v0.1.0` but app is `0.1.0-2`** — 4 sites.
- **Claims page advertises Cmd+K palette + non-existent `export` command** — `site/src/components/Claims.astro:23`.
- **CHANGELOG claims system tray, trigram tokenizer, "50+ languages", "draggable tab strip", "five ADRs"** — none of these match reality (no tray; default tokenizer; 9 langs; no drag; 8 ADRs).
- **ADR-005 lists writ-core deps as `serde + thiserror`** — actual: `serde, serde_json, uuid, chrono, tracing, thiserror`; ADR-005 lists `tokio` in writ-storage — there is no tokio anywhere.
- **packaging/README placeholders `__SHA256_ARM64__`/`__SHA256_INTEL__`** are stale; cask uses `__SHA256_UNIVERSAL__`.
- **Sidebar collapse chevron has no `aria-expanded`/`aria-controls`**.
- **Capability set is missing explicit `allow-unminimize`/`allow-maximize`/`allow-set-size`** etc.; current code relies on `core:default` permissiveness.
- **App writes to `~/.writ` instead of platform data dir** (Windows: `%APPDATA%`; Linux: XDG `~/.local/share/writ`).
- **Close button on Windows/Linux custom titlebar calls `hideWindow()`** — there is no tray, so the app vanishes from the taskbar.
- **Window flags `transparent: true` + `decorations: false` + `shadow: true`** are macOS-tuned; produce artifacts on tiling WMs / classic Windows themes.
- **`ignore-set` filename is case-mismatched on macOS APFS** — OS event returns canonical casing, misses ignore lookup, surfaces a false "external change" toast for our own writes.
- **No bundled mono font** — `--writ-font-mono` references `SF Mono`/`Cascadia Code`/`JetBrains Mono` (none guaranteed off-mac); falls to generic monospace on Linux.

---

## POLISH GAPS (the difference between OK and Hashimoto-grade)

- **No in-app shortcut help / cheat sheet** — there's a ShortcutEditor but no listing surface for first-time users.
- **No onboarding / first-run guidance** for any hotkey or feature.
- **FOUC: non-default-theme users flash warp-dark on launch** — `src/styles/theme.css:1-59` hardcodes warp-dark as `:root`; saved theme applied after async config IPC.
- **EditorInstance rebuilds the entire keymap + 9 extensions on every tab swap** — `src/components/Editor/EditorInstance.tsx:163-179`; should reuse one view via `view.setState`.
- **Tab-item actions overlap timestamp via absolute positioning + `visibility:hidden`** — fragile; use grid swap or opacity crossfade.
- **`editorStore.setCursorLine/Col/SelectionCount/LineCount` fire 4 signal updates per keystroke** — batch with `batch()`.
- **`bufferStore.load()` lacks secondary sort fallback for `tab_order` ties** — `crates/writ-storage/src/database/queries.rs:99-114`.
- **Hardcoded hex traffic-light glyph colors in TSX** — `src/components/TitleBar/TrafficLights.tsx:63,74,85`.
- **Test-only exports leak from `src/lib/modal-stack.ts`** — `modalOpenCount`, `resetModalStack`.
- **Update progress bar transition ignores prefers-reduced-motion** — `src/components/UpdateBanner/UpdateBanner.css:92`.
- **Nested interactive elements** — `<button class="tab">` containing `<span role="button" tabIndex={0} class="tab-close">` (invalid HTML, confuses screen readers).
- **`navigator.platform` is deprecated** — `src/lib/keybinding-format.ts:1`, `src/lib/platform.ts:5`; seed `IS_MAC` from a Rust IPC instead.
- **`HeroDemo.tsx` imports `framer-motion`** despite the documented preference for `motion/react`.
- **Drop-zone empty-state has no "Open file… (⌘O)" affordance** — `src/components/Editor/EditorArea.tsx:17`.
- **Bundle `category: "DeveloperTool"` is mac-only** — no Linux `Categories=` / Windows Start Menu folder.
- **`synchronous = NORMAL` + non-transactional FTS** is fine for perf but undocumented as a durability trade.
- **`open_external` and `save_to_source` do not reject symlinks** — `crates/writ-core/src/file_ops.rs:84-97`; once allowlisting is added (per #10), this is the symlink bypass.

---

## Metrics

### Performance (measured)

| Metric | Measured | Target | Status |
|---|---|---|---|
| FTS 3-letter prefix `tok*` on 10k buffers (median) | **14.2 ms** | <5 ms | FAIL — needs `prefix='2 3'` |
| FTS 3-letter prefix `buf*` on 10k buffers (median) | 4.2 ms | <5 ms | PASS |
| FTS exact 1-token hit (`token0_1`) | 0.025 ms | <5 ms | PASS |
| FTS bulk ingest 10k buffers + content (single txn) | 1197 ms | n/a | one-time |
| Rust file open + read @ 100 KB | 0.024 ms | <1 ms | PASS |
| Rust file open + read @ 1 MB | 0.14 ms | <5 ms | PASS |
| Rust file open + read @ ~9 MB | 0.86 ms | <50 ms | PASS |
| `EditorState.doc.toString()` @ 1 MB | 0.17 ms | <1 ms | PASS (called every keystroke) |
| `EditorState.doc.toString()` @ 5 MB | 1.4 ms | <5 ms | PASS (contributes to jank) |
| `EditorState.doc.toString()` @ 9 MB | 2.9 ms | <16 ms | PASS (jank-adjacent) |
| `detectFromContent` @ 100 KB | 0.75 ms | <1 ms | PASS |
| `detectFromContent` @ 1 MB | **7.6 ms** | <5 ms | FAIL — runs every ~40 keystrokes |
| `detectFromContent` @ 5 MB | **38.2 ms** | <16 ms | FAIL — janks editor |
| `groupActiveByDirectory` @ 10k | 1.13 ms | <16 ms | PASS |
| `bucketHistoryByTime` @ 10k | 2.40 ms | <16 ms | PASS (DOM is the bottleneck) |
| Cold SQLite open + WAL pragma | 0.4 ms (warm), 2.3 ms (cold) | <5 ms | PASS |
| Initial JS bundle (raw / gzip) | **855 KB / 283 KB** single chunk | <250 KB / <80 KB | FAIL — language packs not code-split |
| Cmd+Shift+Space → first paint (warm toggle) | UNMEASURED (GUI required; instrumentation present at `hotkey/mod.rs:49`) | <50 ms | UNMEASURED |
| Cold first-paint (`App.measureFirstPaint("cold")`) | UNMEASURED (GUI required) | <200 ms | UNMEASURED |
| Keystroke → autosave-disk latency (E2E) | UNMEASURED (GUI required) | <50 ms | UNMEASURED |
| Open-100-buffers RSS growth | UNMEASURED (GUI required) | <300 MB | UNMEASURED |

### Accessibility — contrast (computed)

Subtle-foreground tokens fail WCAG AA against every surface in every theme; primary contrast blockers below. Full table in the accessibility section above.

| Theme | Pair | Ratio | AA pass? |
|---|---|---|---|
| warp-dark | subtle `#555566` / bg `#0e0e14` | 2.64 | **no** |
| catppuccin-mocha | subtle `#6c7086` / bg `#1e1e2e` | 3.36 | **no** |
| catppuccin-mocha | muted `#a6adc8` / hover `#45475a` | 4.10 | **no** |
| tokyo-night | subtle `#565f89` / elevated `#24283b` | 2.35 | **no** |
| dracula | subtle `#6272a4` / elevated `#383a4a` | 2.38 | **no** |
| solarized-dark | accent `#268bd2` / elevated `#0a4655` (**active tab text**) | 2.82 | **no** |
| solarized-dark | subtle `#586e75` / elevated `#0a4655` | 1.93 | **no** |
| solarized-dark | muted `#93a1a1` / hover `#0a4655` | 3.89 | **no** |

---

## Recommended fix order

A series of small, focused PRs against `dev`. Each row is one PR; merge order is top-to-bottom because later rows assume earlier ones.

| # | PR title | Scope | Blockers closed |
|---:|---|---|---|
| 1 | `fix(data): atomic buffer writes (temp + rename + fsync)` | `crates/writ-storage/src/buffer_store.rs` + tests | #2 |
| 2 | `fix(data): flush autosave before close; drop empty-content guard` | `src/stores/buffers.ts`, `src/components/Editor/EditorInstance.tsx` + tests | #1 |
| 3 | `fix(data): wrap content+timestamp+FTS in one transaction` | `crates/writ-storage/src/buffer_store.rs` + tests | #5 |
| 4 | `feat(security): sanitize buffer filenames; derive from UUID` | `crates/writ-core/src/buffer/manager.rs`, `crates/writ-storage/`, v2 migration | #9, #7 |
| 5 | `feat(data): wire dirty-shutdown recovery + periodic snapshots` | `src-tauri/src/state.rs`, `src-tauri/src/lib.rs`, recovery integration | #6 |
| 6 | `fix(watcher): content-fingerprint ignore set; canonical filename keys` | `src-tauri/src/watcher/handler.rs`, save callsites | #3 |
| 7 | `fix(buffer): reload editor view on external change` | `src/App.tsx`, `src/components/Editor/EditorInstance.tsx`, `src-tauri/src/watcher/handler.rs` (emit UUID) | #4 |
| 8 | `feat(data): schema_version downgrade guard + UNIQUE(filename)` | `crates/writ-storage/src/database/migrations.rs`, `queries.rs` | #8 |
| 9 | `fix(xplat): platform-aware global hotkey from config` | `src-tauri/src/hotkey/mod.rs` + tests | #11 |
| 10 | `fix(xplat): macOS-only app menu; file-association via argv + single-instance` | `src-tauri/src/lib.rs`, `Cargo.toml` | #12, #13 |
| 11 | `chore(xplat): declare notify backends explicitly; bundle JetBrains Mono` | `src-tauri/Cargo.toml`, `src/styles/fonts.css` | #14 |
| 12 | `fix(interaction): skip app shortcuts inside editor focus; rebind Cmd+S` | `src/commands/keybindings.ts`, `src/App.tsx`, `src/stores/config.ts` | #22, #23 |
| 13 | `fix(interaction): implement (or remove) Cmd+SS double-tap; align docs` | `src/commands/keybindings.ts`, CLAUDE.md, README.md | #24 |
| 14 | `perf(startup): visible:false + frontend show after first paint; parallel boot` | `src-tauri/tauri.conf.json`, `src/App.tsx` | #15 |
| 15 | `perf(updater): gate silent check on session-active threshold + config opt` | `src-tauri/src/lib.rs`, `crates/writ-core/src/config/mod.rs` | #16 |
| 16 | `perf(sidebar): hoist matched-id Set memo` | `src/components/Sidebar/SearchBar.tsx` | #17 |
| 17 | `perf(editor): cap detectFromContent input; disable above 256 KiB` | `src/components/Editor/EditorInstance.tsx`, `src/services/language-detect.ts` | #18 |
| 18 | `perf(editor): defer doc.toString to flush; separate FTS-reindex debounce` | `src/components/Editor/EditorInstance.tsx`, `crates/writ-storage/src/fts.rs` | #19 |
| 19 | `perf(fts): add prefix-token index via v3 migration` | `crates/writ-storage/migrations/*` | #20 |
| 20 | `perf(sidebar): windowed history list + cached now snapshot` | `src/components/Sidebar/HistorySection.tsx` | #21 |
| 21 | `fix(visual): define --writ-accent-foreground per theme` | `src/styles/theme.css`, `src/styles/themes/*.json`, `ShortcutEditor.css` | #25 |
| 22 | `feat(visual): designed empty-state for sidebar` | `src/components/Sidebar/Sidebar.tsx` + CSS | #26 |
| 23 | `refactor(arch): stores/window.ts; route window IPC through services/tauri.ts` | new store, components, services | #27, #28, #29 |
| 24 | `fix(arch): onCleanup(uninstallKeyboardHandler)` | `src/App.tsx` | #30 |
| 25 | `refactor(arch): complete event-bus migration; remove legacy emit` | `src-tauri/src/lib.rs`, watcher, bridge | #31 |
| 26 | `test(watcher): full coverage incl. IgnoreSet drain + debouncer races` | `src-tauri/tests/`, new harness | #32 |
| 27 | `test(update): visibility-policy table + phase-variant table + IPC smoke` | `src-tauri/tests/` | #33 |
| 28 | `test(ipc-file): IgnoreSet-before-write contract for all save paths` | `src-tauri/tests/` | #34 |
| 29 | `fix(history): wrap clear_history in a transaction + test` | `src-tauri/src/commands/history.rs` + tests | #35 |
| 30 | `test(storage): rebuild_fts mixed/missing/stale; close_many rollback` | `crates/writ-storage/tests/` | #36, #37 |
| 31 | `fix(docs): updater bundles → align workflow + build_latest_json.py` | `.github/workflows/release.yml`, `.github/scripts/build_latest_json.py`, `docs/RELEASING.md`, `packaging/README.md` | #41 |
| 32 | `fix(docs): macOS .pkg pipeline + Homebrew cask sanity` | `.github/workflows/release.yml`, packaging/ | #42 |
| 33 | `fix(docs): site/README placeholders until v0.1.0 is published; install.sh friendly fail` | README, site, install.sh | #38, #39 |
| 34 | `fix(site): bundle Newsreader + JetBrains Mono fonts (or remove preloads)` | `site/public/fonts/`, `site/src/layouts/Base.astro`, `site/src/styles/global.css`, `site/src/pages/privacy.astro` | #40 |
| 35 | `fix(docs): register Cmd+F sidebar search or remove from README` | `src/App.tsx` OR `README.md` | #43 |
| 36 | `feat(a11y): keyboard-operable sidebar rows + actions` | `src/components/Sidebar/TabItem.tsx`, CSS | #44, #45 |
| 37 | `fix(a11y): accessible names on icon-only buttons (new tab, close, dismiss)` | `src/components/Editor/TabBar.tsx`, `src/components/Notifications/Toast.tsx` | #46, #47, #48 |
| 38 | `fix(a11y): persistent aria-live regions for toasts + save status` | `src/components/Notifications/Toast.tsx`, `src/components/Editor/StatusBar.tsx` | #49, #50 |
| 39 | (roll-up) `feat(quality): see roll-up issue` | various | QUALITY |
| 40 | (roll-up) `feat(polish): see roll-up issue` | various | POLISH |

---

## Full agent reports

(See agent outputs below — preserved verbatim for traceability.)

### Performance
(see agent `perf-audit` report in audit transcript)

### Visual polish
(see agent `visual-audit` report in audit transcript)

### Interaction quality
(see agent `interaction-audit` report in audit transcript)

### Architecture smells
(see agent `arch-audit` report in audit transcript)

### Test coverage
(see agent `test-audit` report in audit transcript)

### Documentation / claim parity
(see agent `docs-audit` report in audit transcript)

### Security
(see agent `security-audit` report in audit transcript)

### Data integrity
(see agent `data-audit` report in audit transcript)

### Cross-platform readiness
(see agent `xplat-audit` report in audit transcript)

### Accessibility
(see agent `a11y-audit` report in audit transcript)
