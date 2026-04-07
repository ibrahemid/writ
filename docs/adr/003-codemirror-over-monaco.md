# ADR-003: CodeMirror 6 Over Monaco

**Status:** Accepted
**Date:** 2026-03-27

## Context

Writ needs an embeddable code editor component. Two serious options exist: Monaco (the editor
powering VS Code) and CodeMirror 6 (a ground-up rewrite of the CodeMirror library released
in 2021).

Monaco is well-known, ships with VS Code, and has built-in IntelliSense, multi-cursor, and
a rich extension API. However, it carries significant weight: the full Monaco bundle is roughly
900KB–1.5MB uncompressed and requires web workers to function. In a Tauri webview, web worker
support exists but adds integration complexity, and the bundle size contributes directly to
cold-start parse time.

Writ's use case is not an IDE. The target user is writing prose and lightweight markup, not
navigating large TypeScript projects with type-checked autocompletion. The editor feature set
needed is: syntax highlighting for 20–50 languages, soft wrap, multiple cursors, search/replace,
and a clean extension API for themes.

## Decision

Use CodeMirror 6 as the editor component.

Key factors:

- **Bundle size**: CodeMirror 6 core (editor state + view) is approximately 45KB gzipped.
  Individual language packages are ~5–20KB each and loaded on demand. Monaco's minimum viable
  bundle is 15–20x larger.
- **No web workers required**: CodeMirror 6 performs all tokenization and state transitions
  synchronously on the main thread. This eliminates a category of Tauri webview integration
  issues and simplifies the build configuration.
- **Language support**: CodeMirror ships first-party packages for 50+ languages via
  `@codemirror/lang-*`. The `@codemirror/language-data` package provides lazy-loaded language
  detection.
- **Modular architecture**: The extension system composes state fields, decorations, and
  event handlers. Adding features (word count, line numbers, vim bindings via `@replit/codemirror-vim`)
  is additive and tree-shakeable.
- **RTL and Unicode**: CodeMirror 6 has first-class bidirectional text and Unicode support.
  Monaco's RTL support is present but historically less complete.

## Consequences

**Positive:**
- Minimal parse-and-execute cost at startup in the Tauri webview.
- No web worker orchestration; editor initializes inline.
- Extension API is well-suited to Writ's needs (themes, keymaps, decorations for search results).
- Active maintenance with a clear versioning policy.

**Negative / risks:**
- **No built-in IntelliSense**: CodeMirror 6 has an autocomplete extension, but wiring it to
  a language server requires manual integration work. For Writ's current scope this is not
  needed, but it is a gap to acknowledge if IDE features are added later.
- **More assembly for complex IDE features**: Monaco ships multi-cursor, find-in-file, breadcrumbs,
  and diff view out of the box. With CodeMirror 6 these are either first-party packages to
  integrate or custom implementations.
- **API complexity**: The CodeMirror 6 extension system is powerful but has a steeper learning
  curve than dropping in a Monaco editor instance. Initial setup requires understanding state
  fields, transactions, and the decoration model.
