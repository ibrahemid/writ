# Toggle Latency Baseline

The "always-ready" promise of Writ's global hotkey (Cmd+Shift+Space) is a
load-bearing piece of the product story. This document defines what we measure,
how we measure it, and what the baseline numbers are on a developer-class macOS
machine. Use the numbers here as the regression target when changes touch the
window lifecycle, the SolidJS root mount, the CodeMirror constructor, or any
code that runs between hotkey-fire and the first painted frame.

## What is measured

Two distinct paths are instrumented:

**Cold toggle.** The webview process is not yet running. The user invokes Writ
for the first time after launch (or after a full quit). The measurement covers
the SolidJS root `onMount` callback through the next
`requestAnimationFrame` paint. The Rust side of cold start is dominated by
process spawn + WebView2 / WebKitGTK init, which the existing
`tauri::Builder::default().setup` instrumentation will surface in
`~/.writ/logs/`.

**Warm toggle.** The webview is alive but the window is hidden (or
unfocused/minimized). The user presses Cmd+Shift+Space. The measurement
covers two pieces:

- *Rust side* — from the moment the global-shortcut handler captures
  `Instant::now()` through every `window.show()` / `window.set_focus()` /
  `window.unminimize()` call returning. Logged as `rust_elapsed_us`.
- *Frontend side* — from receipt of the `writ://window-shown` Tauri event
  through the next `requestAnimationFrame` paint. Logged as `elapsed_ms`,
  paired with `rust_elapsed_us` in the same record so the two halves can be
  summed.

Both modes call the same `report_first_paint(elapsed_ms, mode, rust_elapsed_us)`
IPC, which logs a single structured `info!` line tagged with `mode`.

## How to capture

1. Build and launch:

   ```bash
   cargo tauri dev
   ```

2. Tail the runtime log:

   ```bash
   tail -F ~/.writ/logs/writ.log | grep "toggle-latency"
   ```

3. *Cold:* observe the line emitted on first launch. One run per process.
4. *Warm:* with the app already running, hide the window (Cmd+Shift+Space or
   click outside). Wait for any in-flight WebKit work to settle (~1 second is
   plenty). Press Cmd+Shift+Space. Observe the next log line. Repeat at least
   five times and discard the first (always slower — JIT and cache warm-up).

The reported `elapsed_ms` is the frontend half; `rust_elapsed_us` is the Rust
half. Total perceived latency is approximately their sum (a few microseconds of
IPC delivery are not separately measured).

## Targets

These are the targets that future PRs should not regress against. They are
intentionally tighter than what users will notice — a worse number on a
specific change should be a deliberate trade-off, surfaced in the PR, not a
silent drift.

| Mode | Target (p50) | Hard fail (p95) |
| ---- | -----------: | --------------: |
| Cold (frontend onMount → first paint) | < 100 ms | 250 ms |
| Warm Rust (hotkey → show return) | < 5 ms | 20 ms |
| Warm frontend (window-shown → first paint) | < 30 ms | 80 ms |

The cold target is roomy because the WebView itself dominates and we don't
control its boot time. The warm targets are tight because the warm path is
what users feel every day.

## Baseline run

Platform: macOS Darwin 25.5.0, MacBook Air M2 (developer-class).
Build: `cargo tauri dev` profile (`dev`, not `release`). Release-build numbers
will be tighter; this document captures the dev-build baseline so day-to-day
regressions are visible.

> **Numbers to be filled on first capture run.** Run the steps above, append
> the five warm-toggle samples and the single cold-toggle sample, then commit
> an update to this section. Until that capture lands, regression comparison
> is qualitative.

```
cold first paint: TBD ms
warm rust elapsed: TBD µs
warm frontend first paint: TBD ms (five samples, drop the first)
```

## How to regress against this

When reviewing a PR that touches:

- `src-tauri/src/hotkey/mod.rs`
- `src-tauri/src/window_state.rs`
- `src-tauri/src/commands/window.rs`
- `src/App.tsx` `onMount`
- `src/components/Editor/EditorInstance.tsx` initialization
- `src/stores/buffers.ts` `load`

…re-capture the warm and cold numbers using the same procedure. If either
moves past the *Hard fail* column, request explicit justification in the PR
body.

## What is intentionally NOT measured here

- Time from physical keypress to `tauri-plugin-global-shortcut` dispatch.
  This is in OS hands and not actionable.
- Time from `app.emit` to the JS `listen` callback firing. Tauri docs claim
  sub-millisecond on macOS; we trust that until evidence says otherwise.
- macOS Spaces / Mission Control transitions. The hotkey is global, so the
  cost of crossing Spaces is highly user-dependent and not a Writ regression
  surface.
