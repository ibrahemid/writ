# ADR-005: Cargo Workspace Split

**Status:** Accepted
**Date:** 2026-03-27

## Context

Without explicit structure, Rust projects tend toward a single crate that gradually accumulates
all logic. For Writ, the risk is that `src-tauri` — the crate that must import `tauri` —
becomes the home for business logic that has no business depending on the Tauri framework.

The specific failure mode: a developer adds a buffer-processing function directly into a Tauri
command handler because it is convenient. Six months later, unit testing that function requires
spinning up a Tauri application context. The domain logic has become untestable in isolation.

A secondary risk is the inverse: future work on `writ-core` inadvertently adds a dependency
on `tauri` (perhaps transitively through a utility crate), coupling the domain model to a
specific desktop framework version.

Both failure modes are invisible without a structural enforcement mechanism. Code review
can catch them, but code review is inconsistent. The compiler is not.

## Decision

Structure the project as a Cargo workspace with four crates, each with explicit and intentionally
limited dependencies:

```
writ-core       — no framework deps; serde, serde_json, uuid, chrono, tracing, thiserror, sha2
writ-storage    — depends on writ-core; adds rusqlite, toml
writ-plugin     — depends on writ-core; defines extension boundary
src-tauri       — depends on all three; the only crate that lists tauri as a dependency
```

The `[workspace]` `Cargo.toml` does not add `tauri` as a workspace-level dependency. Each
crate's `Cargo.toml` lists only what it directly needs. If any developer adds `tauri` to
`writ-core/Cargo.toml`, the logical contradiction becomes visible immediately — the pure domain
crate now depends on a GUI framework — and will be caught in review or by CI checking that
`writ-core` compiles with `--no-default-features` and no Tauri feature flags.

Architecture tests in `src-tauri` assert at compile time (via `#[cfg]` guards on test modules)
that `writ-core` does not re-export Tauri types.

## Consequences

**Positive:**
- `writ-core` and `writ-storage` are testable with `cargo test -p writ-core` and
  `cargo test -p writ-storage` with no Tauri runtime present. CI runs these tests on every push
  without needing a display server or window manager.
- If Writ were ever ported to a different shell (e.g., a web server, a TUI, or a future
  Tauri v3), `writ-core` and `writ-storage` carry over unchanged. Only the adapter crate is
  replaced.
- The dependency graph is a documentation artifact. Reading `writ-core/Cargo.toml` tells you
  immediately what the domain model is allowed to use.
- Incremental compilation is faster. Changes to `src-tauri` do not force recompilation of
  `writ-core`. Changes to `writ-core` do not trigger re-linking of Tauri.

**Negative / risks:**
- **More `Cargo.toml` files to maintain**: Shared dependencies (serde version, chrono version)
  must be kept in sync across crates. Mitigated by using `[workspace.dependencies]` to declare
  versions once and inherit them in member crates with `serde = { workspace = true }`.
- **Cross-crate refactoring is noisier**: Moving a type from `writ-storage` to `writ-core`
  requires updating imports in all crates that referenced it. This is intentional friction —
  the cost of moving something down the dependency stack should be non-trivial.
- **Onboarding explanation required**: New contributors need to understand why there are four
  crates instead of one. The payoff (testable domain logic, enforced boundaries) is not
  obvious until they try to write a unit test without it.
