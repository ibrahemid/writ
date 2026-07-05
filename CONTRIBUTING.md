# Contributing to Writ

Thank you for your interest in contributing. This document covers everything you need to get a working development environment, understand the codebase, and submit quality pull requests.

## Prerequisites

- **Rust** 1.77+, install via [rustup.rs](https://rustup.rs)
- **Node.js** 20+, via [nodejs.org](https://nodejs.org) or a version manager
- **pnpm** 9+: `npm install -g pnpm`
- **Tauri CLI prerequisites** for your platform: [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/)
  - macOS: Xcode Command Line Tools
  - Linux: `webkit2gtk`, `libayatana-appindicator3-dev`, and build essentials
  - Windows: Visual Studio Build Tools with C++ workload, WebView2

## Building from Source

```bash
git clone https://github.com/ibrahemid/writ.git
cd writ
pnpm install
cargo tauri dev          # development (hot-reload)
cargo tauri build        # production bundle
```

## Running Tests

```bash
cargo test --workspace           # all Rust unit and integration tests
pnpm exec tsc --noEmit           # TypeScript type-checking
pnpm build                       # full frontend build (catches bundler errors)
```

Run all three before submitting a pull request. CI runs the same checks.

## Project Structure

```
writ/
├── crates/
│   ├── writ-core/       # Pure Rust business logic (no I/O, no Tauri)
│   ├── writ-storage/    # SQLite persistence, FTS5, migrations
│   └── writ-plugin/     # Plugin API boundary types and trait definitions
├── src-tauri/           # Tauri adapter: IPC commands, app lifecycle, file watcher
└── src/                 # SolidJS frontend: components, stores, services
```

The crate separation is intentional and compiler-enforced. `writ-core` must never import `tauri` or `writ-storage` directly. See [ADR-005](docs/adr/005-cargo-workspace-split.md).

## Code Style

**Rust**

```bash
cargo fmt --all                          # format
cargo clippy -- -D warnings              # lint (warnings are errors)
```

All clippy warnings must be resolved before merging. Do not add `#[allow(...)]` annotations without a comment explaining why.

**TypeScript / SolidJS**

Follow the existing patterns in `src/`. Type everything; `any` is not permitted. The TypeScript config is strict.

**Comments**

Do not add explanatory comments to self-documenting code. Only comment complex algorithms or non-obvious platform workarounds, with enough context to understand why the approach was chosen.

## Commit Conventions

```
type(scope): short description
```

Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`

Examples:

```
feat(storage): add FTS5 trigram tokenizer for partial-word search
fix(ipc): handle concurrent close_buffer calls without panic
docs(adr): record decision to use SolidJS over React
```

- Imperative mood, lowercase, no trailing period
- Keep commits atomic: one logical change per commit
- Reference issue numbers in the body when applicable

## Pull Request Process

1. Fork the repository and create a branch from `main`.
2. Make your changes with passing tests and no clippy warnings.
3. Update `CHANGELOG.md` under `[Unreleased]` with a summary of your change.
4. Open a pull request against `main`. Fill in the PR template.
5. A maintainer will review within a few business days.
6. Address review feedback in new commits (do not force-push during review).
7. Once approved, a maintainer will squash-merge.

## Architecture Decision Records

Significant design decisions are captured as ADRs in [docs/adr/](docs/adr/). If your contribution changes an established pattern or introduces a new architectural choice, add or update an ADR as part of your PR.

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating you agree to uphold it.
