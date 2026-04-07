# ADR-002: SolidJS Over React

**Status:** Accepted
**Date:** 2026-03-27

## Context

Writ's webview UI needs a frontend framework. The team already works with React in other
contexts, making it the path of least friction. However, Writ's UI has specific characteristics
that make React's design trade-offs worth reconsidering:

- The editor area re-renders in response to cursor movement, selection changes, and document
  mutations — all high-frequency events.
- The UI must remain responsive while CodeMirror is processing keystrokes. Any framework overhead
  on the hot path is directly perceptible as latency.
- The target environment is a Tauri webview, not a browser serving millions of users. Bundle size
  affects initial load time and cold-start feel, not CDN cache-hit rate.

React's virtual DOM diffing adds a reconciliation step between state changes and DOM updates.
For most CRUD applications this overhead is imperceptible. For a text editor, it is not.

## Decision

Use SolidJS as the frontend framework. SolidJS compiles reactive expressions to direct DOM
operations at build time. There is no virtual DOM and no reconciliation pass at runtime.

Key factors:

- **Runtime size**: SolidJS ships ~7KB (gzipped). React + ReactDOM is ~45KB gzipped. In a
  Tauri webview that is not cached across page loads, this difference is felt at startup.
- **Reactivity model**: SolidJS signals track dependencies at the expression level. A signal
  update triggers only the specific DOM node that depends on it. There is no component tree
  re-render to bubble through.
- **Performance ceiling**: In framework benchmarks (js-framework-benchmark), SolidJS
  consistently outperforms React on DOM update throughput. For an editor UI this is the
  correct optimization target.
- **Mental model alignment**: SolidJS components run once and set up reactive subscriptions.
  This is closer to how CodeMirror's own state management works, reducing the impedance mismatch
  between the editor core and the surrounding UI.

## Consequences

**Positive:**
- Zero reconciliation overhead on high-frequency state updates (cursor position, status bar,
  search highlights).
- Smaller initial parse-and-execute budget in the webview on cold start.
- Reactive primitives (`createSignal`, `createStore`, `createMemo`) map naturally to the
  editor's event stream without needing `useMemo`/`useCallback` discipline to avoid re-renders.

**Negative / risks:**
- **Smaller ecosystem**: The SolidJS component library and tooling ecosystem is substantially
  smaller than React's. Generic UI components (modals, tooltips, tree views) will often need
  to be written from scratch rather than pulled from a library.
- **Fewer familiar developers**: Candidates with SolidJS experience are rarer. Onboarding
  requires learning the reactivity model, which has subtle differences from React hooks
  (e.g., destructuring props breaks reactivity).
- **Fewer examples for Tauri integration**: Most Tauri community examples use React or Vue.
  SolidJS-specific patterns for `invoke()` and event handling are less documented, requiring
  the team to establish its own conventions.
