# ADR-001: Tauri Over Electron

**Status:** Accepted
**Date:** 2026-03-27

## Context

Writ needs a cross-platform desktop shell that can host a web-based UI on macOS, Windows, and
Linux. The dominant option in this space is Electron. Electron bundles a full copy of Chromium and
Node.js into every application, producing binaries that are 150MB+ on disk and consuming 200MB+
of RAM at idle before the application has done any work. For a text editor positioned as
lightweight, shipping half a browser is a contradiction.

The two realistic alternatives at evaluation time were Tauri v2 and a native-only approach
(Swift/AppKit + WinUI + GTK). A native approach would require three separate UI codebases,
which is not viable for a small team.

## Decision

Use Tauri v2 as the desktop shell. Tauri renders the UI using the platform's existing system
webview — WKWebView on macOS, WebView2 on Windows, WebKitGTK on Linux — and exposes a Rust
backend that the frontend communicates with over a typed IPC bridge.

Key factors:

- **Binary size**: Tauri applications ship at 5–15MB versus 150MB+ for Electron. This is a
  categorical difference, not a marginal optimization.
- **Memory**: Tauri idle RAM usage is 30–60MB. Electron idles at 200MB+ before any app code runs.
- **Security model**: Tauri's capability system grants only the permissions explicitly declared.
  Electron's Node.js integration in the renderer process is an inherently larger attack surface.
- **Ecosystem health**: Tauri v2 has 88K+ GitHub stars, is foundation-backed (CrabNebula), and
  has an active release cadence with a stable plugin ecosystem.
- **Rust backend**: The host process is Rust, which aligns with the goal of writing the core
  editor logic in Rust without a separate inter-process protocol.

## Consequences

**Positive:**
- Dramatically smaller install footprint and faster cold-start time.
- Rust backend shares types with `writ-core` and `writ-storage` directly; no FFI boundary
  or serialization layer between the shell and the domain logic.
- Security capability model limits blast radius of compromised webview content.

**Negative / risks:**
- **WebKitGTK fragmentation on Linux**: Different distributions ship different versions of
  WebKitGTK. CSS and JavaScript behavior is less consistent than Chromium. We accept this and
  test against Ubuntu LTS and Fedora explicitly.
- **Wayland hotkey limitations**: Global hotkeys via WebKitGTK on Wayland are restricted by the
  compositor. Editor-level keybindings that are scoped to the window are unaffected; only
  system-wide shortcuts are constrained.
- **Rust learning curve**: Team members without prior Rust experience will need ramp-up time for
  the backend. Mitigated by confining Tauri-specific code to `src-tauri` and keeping `writ-core`
  as pure, approachable Rust.
- **WebView2 runtime on Windows**: Windows users need WebView2 installed. It ships pre-installed
  on Windows 11 and most Windows 10 systems via Windows Update, but edge cases exist.
