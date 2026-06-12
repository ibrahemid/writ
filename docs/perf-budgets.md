# Performance Budgets

This document defines, justifies, and explains how to re-baseline every
performance budget enforced by `scripts/perf-gate.sh`.

## How the gate works

`scripts/perf-gate.sh` runs the budget integration tests in **release mode**
with `WRIT_PERF_GATE=1`. The tests skip when that variable is absent so that
`cargo test --workspace` stays fast and deterministic in debug builds.

```
bash scripts/perf-gate.sh            # budget tests only
bash scripts/perf-gate.sh --benches  # budget tests + criterion benches
```

The gate exits non-zero on any budget violation. Run it before tagging a
release or after a change that touches a transform, the FTS layer, or buffer
I/O.

---

## writ-plugin: text transform budgets

**Test file:** `crates/writ-plugin/tests/perf_budget.rs`

**What is measured:** median wall time (9-sample) of each registered transform
applied to a deterministic 100 KB mixed markdown/code fixture. The fixture is
built from a fixed set of patterns without random input, so timing variance
comes only from machine state, not from input variance.

**Budget: 100 ms median per transform on a 100 KB input**

Rationale:

- ADR-006 specifies "under 16ms on a 100KB input on a developer-class laptop
  (one frame at 60Hz)" as the design target, measured in release mode.
- The gate budget is set at **100 ms** (6x the ADR-006 design target) to give
  generous headroom across slower CI runners and debug hardware without
  masking real quadratic regressions. A transform that turns O(n) into O(n^2)
  on a 100KB input would exceed this bound by a large margin.
- The existing `builtin_perf_tests.rs` already asserts 50ms in debug mode as a
  softer sanity check. The gate enforces the harder release-mode bound.

**Transforms covered:** every transform registered by `register_builtins`,
including `tidy_whitespace` (the curated composite from ADR-012).

### Re-baselining

If a new transform is legitimately slower than 100ms on 100KB (e.g., because
it uses a more expensive algorithm for better correctness), evaluate whether
the algorithm is appropriate. If yes, raise the per-transform budget in
`perf_budget.rs:BUDGET_MS` and update this document with the new number and
the reason.

---

## writ-storage: FTS search latency

**Test file:** `crates/writ-storage/tests/perf_budget.rs`

**What is measured:** median wall time (9-sample) of `BufferStore::search`
over a seeded corpus of 500 buffers with varied content sizes (512–4096 bytes
per buffer), using four representative queries.

**Budget: 200 ms median per query over 500 buffers**

Rationale:

- FTS5 queries on a warm SQLite connection with 500 rows are expected to
  complete in single-digit milliseconds on any developer machine. The 200ms
  budget is intentionally very generous to avoid flakiness from I/O startup
  cost on the first query in a test process.
- The corpus is 500 buffers because that is a realistic upper bound for a
  power user's scratch session. Beyond this count the user would typically use
  a project-level search tool, not Writ's buffer FTS.
- Cold-start noise (opening the SQLite file, page cache miss on first query)
  is absorbed by the 9-sample median. The test suite warms the corpus before
  timing.

### Re-baselining

If the corpus size or query shape changes materially, update `CORPUS_SIZE` and
`FTS_BUDGET_MS` in `perf_budget.rs` and revise this document with the new
numbers and rationale.

---

## writ-storage: buffer save/load round-trip

**Test file:** `crates/writ-storage/tests/perf_budget.rs`

**What is measured:** median wall time (9-sample) of `save_content` and
`read_content` on a single 4 KB buffer.

**Budget: 50 ms median per operation**

Rationale:

- 4KB represents a typical scratch buffer or short note. Autosave fires on
  every keypress debounce (500ms default); a 50ms save budget leaves 90% of
  the autosave window free for other work.
- File I/O on a local SSD completes in well under 1ms; the 50ms budget
  accounts for kernel scheduling overhead and the SQLite timestamp update that
  accompanies every save. Any regression that pushes this above 50ms indicates
  a structural change (e.g., unnecessary fsync, N+1 query).

### Re-baselining

If the buffer store gains a new on-write step (e.g., checksumming, encryption)
that legitimately raises wall time, update `ROUND_TRIP_BUDGET_MS` and document
the new step here.

---

---

## writ-storage: large-file open/read round-trip (10 MiB)

**Test file:** `crates/writ-storage/tests/perf_budget.rs`

**What is measured:** median wall time (9-sample) of `read_content` on a
single buffer whose backing file is just above `THRESHOLD_NORMAL_BYTES`
(5 MiB + 1 byte). FTS indexing is skipped for this tier.

**Budget: 500 ms median per read**

Rationale:

- A 5 MiB file read on a local SSD completes in under 5ms; the 500ms budget
  absorbs I/O scheduling overhead and allows for slower hardware without
  masking regressions. A 10x regression (e.g., accidentally enabling FTS
  indexing for large files) would push a 5 MiB read well above this bound.

### Re-baselining

Raise `OPEN_10MB_BUDGET_MS` if the backing file format gains a mandatory
transformation step (e.g., on-disk encryption). Document the step.

---

## writ-storage: large-file open/read round-trip (50 MiB)

**Test file:** `crates/writ-storage/tests/perf_budget.rs`

**What is measured:** median wall time (9-sample) of `read_content` on a
single buffer at `THRESHOLD_LARGE_BYTES` (50 MiB).

**Budget: 4000 ms median per read**

Rationale:

- 50 MiB on a local SSD is typically 50–200ms; the 4000ms budget gives a 20x
  margin for virtual filesystems, network mounts, and slow test hardware.
  A structural regression (e.g., copying the content twice in memory) on a
  50 MiB read would be visible as a 2× slowdown well below this ceiling, so
  the budget is a safety net for worst-case hardware rather than a tight
  correctness check.

### Re-baselining

Raise `OPEN_50MB_BUDGET_MS` only when a new mandatory read step is added and
its cost is justified. Document the step and the new expected floor.

---

## writ-core: hex dump generation (10 MiB)

**Test file:** `crates/writ-storage/tests/perf_budget.rs`

**What is measured:** median wall time (9-sample) of `generate_hex_dump` on a
10 MiB input (the `HEX_DUMP_MAX_BYTES` cap).

**Budget: 1000 ms median**

Rationale:

- `generate_hex_dump` is a pure Rust function with a single pass over the
  input and one `String::with_capacity` pre-allocation. A 10 MiB input
  produces ~655K rows × ~80 bytes = ~52 MB of output. On a developer laptop
  this completes in roughly 100–300ms in release mode. The 1000ms budget
  provides a 3–10× margin for debug builds and slower hardware.

### Re-baselining

Raise `HEX_DUMP_10MB_BUDGET_MS` only if a formatting change (e.g., adding
colour escape codes, Unicode rendering) legitimately increases output size.

---

## Fixture determinism guarantee

Both the transform fixture and the storage corpus are fully deterministic:
no random number generators, no time-seeded data, no OS entropy. A test
asserts fixture/corpus determinism explicitly. If you add randomness anywhere
in a fixture builder, add an accompanying assertion that two calls produce
identical output.
