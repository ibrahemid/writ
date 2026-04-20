## Description

<!-- Brief description of what this PR changes and why. -->

## Type of Change

- [ ] Bug fix (non-breaking change that fixes an issue)
- [ ] New feature (non-breaking change that adds functionality)
- [ ] Breaking change (fix or feature that would change existing behavior)
- [ ] Refactor (no functional change)
- [ ] Documentation
- [ ] Chore / tooling

## Quality Gates

All boxes must be checked before a maintainer will review.

- [ ] `cargo test --workspace` passes locally
- [ ] `cargo clippy --workspace -- -D warnings` is clean
- [ ] `cargo fmt --check` is clean
- [ ] `npx tsc --noEmit` passes (if frontend changes)
- [ ] `pnpm build` succeeds (if frontend changes)

## Architecture Compliance

Confirm the change respects the rules in `CLAUDE.md`:

- [ ] Cargo workspace boundaries respected (`writ-core` has no Tauri dependency, `src-tauri` is the only Tauri adapter).
- [ ] `src/services/tauri.ts` is still the only file that imports `@tauri-apps/api`.
- [ ] `src/services/events.ts` is still the only file that imports `@tauri-apps/api/event`.
- [ ] Components call stores, stores call services, services call Tauri. No layers skipped.
- [ ] No `document.querySelector` / ad-hoc `document.addEventListener` added outside `onMount` / `onCleanup`.
- [ ] New Rust functions have tests. New IPC commands have coverage.

## Related Issues

<!-- Link issues this PR closes, e.g. "Closes #123". Leave blank if none. -->

## Screenshots / Recordings

<!-- Attach before/after screenshots or a short clip for any UI change. -->

## Notes for Reviewers

<!-- Anything a reviewer should focus on, tricky tradeoffs, follow-up work. -->
