# Performance Budgets

This document defines, justifies, and explains how to re-baseline every
performance budget enforced by `scripts/perf-gate.sh`.

## How the gate works

`scripts/perf-gate.sh` runs every budget test with `WRIT_PERF_GATE=1`: the Rust
ones in **release mode**, the frontend one under vitest, which has no such
mode. Both read that same variable and skip without it, so
`cargo test --workspace` and `pnpm test` stay fast and deterministic. One Rust
probe (`builtin_perf_tests.rs::timing_100kb_builtins`) is held back by
`#[ignore]` instead; the gate runs it with `--ignored`.

```
bash scripts/perf-gate.sh            # budget tests only
bash scripts/perf-gate.sh --benches  # budget tests + criterion benches
```

The gate exits non-zero on any budget violation. Run it before tagging a
release or after a change that touches a transform, the notes index, or note
I/O. The storage budgets run one test at a time (`--test-threads=1`): two
budgets that each walk thousands of notes would otherwise time each other.

---

## Test projects (frontend)

`vite.config.ts` splits the vitest suite into two projects, by path:

- **mount**: `src/__tests__/components/**`, `src/__tests__/editor/**`,
  `src/__tests__/integration/**`, and the co-located
  `src/components/**/__tests__/**`. These files mount a component or an editor
  view, which on a busy machine can take longer than vitest's 5 s default, so
  the project sets `testTimeout: 15_000`.
- **unit**: everything else, on the 5 s default. A unit test that needs more
  than 5 s is a real finding, not a scheduling artifact.

The two include sets are disjoint and cover the whole suite: every test file
runs in exactly one project. A new mounting test placed outside those paths
belongs in the `MOUNT_TESTS` globs.

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
- `builtin_perf_tests.rs` carries a second, tighter probe: 50 ms per transform
  on its own 100 KB input, measured once per transform after a warm-up run.
  It is `#[ignore]`d, so it never runs in a debug `cargo test --workspace`
  (where a busy machine misses 50 ms for reasons unrelated to the code) and
  runs only from `scripts/perf-gate.sh`, or by hand with
  `cargo test --release -p writ-plugin --test builtin_perf_tests -- --ignored --nocapture`.
  Its default-suite companion, `every_builtin_holds_its_contract_on_a_100kb_input`,
  keeps the untimed half in every run: determinism, idempotence, and the
  trailing-whitespace and final-newline contracts on the same input.

**Transforms covered:** every transform registered by `register_builtins`,
including `tidy_whitespace` (the curated composite from ADR-012).

### Re-baselining

If a new transform is legitimately slower than 100ms on 100KB (e.g., because
it uses a more expensive algorithm for better correctness), evaluate whether
the algorithm is appropriate. If yes, raise the per-transform budget in
`perf_budget.rs:BUDGET_MS` and update this document with the new number and
the reason.

---

## frontend: language detection input cap

**Test file:** `src/__tests__/services/language-detect.test.ts`
(`bails out of the huge JSON body in under 16ms`)

**What is measured:** wall time of `detectFromContent` on a JSON body whose
closing brace sits past the 64 KiB detection cap, the input a naive
implementation would hand to `JSON.parse` whole.

**Budget: 16 ms (one frame at 60Hz)**

Rationale:

- Detection runs on the open path, in front of the first paint of a document,
  so it has one frame to spend however large the document is. The structural
  check that makes this hold (first/last character, before any parse) is what
  the budget guards.
- The assertion is skipped unless `WRIT_PERF_GATE=1`, so a loaded machine
  cannot fail `pnpm test`; the untimed half of the same property (a huge JSON
  body is never detected as JSON) runs in every suite. `scripts/perf-gate.sh`
  runs the timed half, or by hand:
  `WRIT_PERF_GATE=1 pnpm exec vitest run src/__tests__/services/language-detect.test.ts`.

### Re-baselining

The budget is the frame, not a machine number: it does not move. If detection
legitimately needs more work per open, cut the input further rather than
raising the bound.

---

## writ-storage: FTS search latency

**Test file:** `crates/writ-storage/tests/perf_budget.rs` (`fts_search_budget`)

**What is measured:** median wall time (9-sample) of `NotesIndexStore::count`
over a folder of 500 generated notes (512–4096 bytes of text each, with
frontmatter, a heading, a tag and two links) walked into the index, using four
representative queries.

**Budget: 200 ms median per query over 500 notes**

Rationale:

- FTS5 queries on a warm SQLite connection over 500 rows complete in
  single-digit milliseconds on any developer machine. The 200 ms budget is
  generous on purpose, to avoid flakiness from I/O startup cost on the first
  query in a test process.
- Cold-start noise (opening the SQLite file, page cache miss on first query)
  is absorbed by the 9-sample median.

### Re-baselining

If the corpus size or query shape changes materially, update `CORPUS_SIZE` and
`FTS_BUDGET_MS` in `perf_budget.rs` and revise this document with the new
numbers and rationale.

---

## writ-storage: FTS prefix-search latency

**Test file:** `crates/writ-storage/tests/perf_budget.rs` (`fts_prefix_search_budget`)

**What is measured:** median wall time (9-sample) of `NotesIndexStore::count`
over the same 500-note folder, using the quoted prefix queries the live
search-as-you-type path actually issues (`writ_core::search::to_prefix_match`,
e.g. `"rus"*`). Each query is asserted to match at least one row before
timing, so a silently-broken index cannot pass by matching nothing.

**Budget: 50 ms median per prefix query over 500 notes**

Rationale:

- The prefix index (`prefix='2 3 4'`, created by migration 040 the way 030
  created it for the old buffer table) turns a 2–4 character prefix term from
  a full term-list scan into a seek. The 2026-05-22 audit measured the
  *unindexed* scan at 14 ms median over 10k rows; with the index the same
  query is sub-millisecond. The 50 ms budget is tighter than the exact-match
  `FTS_BUDGET_MS` (200 ms) on purpose: it is the regression guard that fails
  if the prefix index is dropped or the prefix query stops resolving.

### Re-baselining

Raise `FTS_PREFIX_BUDGET_MS` only if the prefix configuration or tokenizer
changes in a way that legitimately raises latency, and document the change.

---

## writ-storage: guarded save round trip

**Test file:** `crates/writ-storage/tests/perf_budget.rs` (`guarded_save_round_trip_budget`)

**What is measured:** median wall time (9-sample) of the save the app makes,
`BufferStore::save_to_source_without_index` with the state the previous save
returned (the guarded write, `guarded::write_note_guarded`, judged against what
Writ last saw on disk, ADR-028 §5), and of `read_content`, on a 4 KB note in a
notes folder. The two timed texts alternate, so every save writes: the guard
leaves alone a file that already holds the incoming bytes. The index's half of
a save is the next budget. The criterion bench `buffer_round_trip/save_4kb`
times the same save.

**Budget: 50 ms median per operation**

Rationale:

- 4 KB is a typical note. Autosave fires on the keypress debounce (500 ms
  default); a 50 ms save leaves 90% of the window free.
- The write is a temp file, an fsync and a rename beside the note, plus the
  read the guard compares against. On a local SSD that is a few milliseconds;
  a regression to 50 ms means a structural change (a second fsync, a read of
  the whole folder).

### Re-baselining

If the guarded write gains a mandatory step (a checksum, an encryption pass),
update `ROUND_TRIP_BUDGET_MS` and document the step here.

---

## writ-storage: per-save index upsert at 5,000 notes

**Test file:** `crates/writ-storage/tests/perf_budget.rs` (`per_save_upsert_at_5000_notes_stays_under_budget`)

**What is measured:** the slowest of 21 `NotesIndexStore::upsert` calls on one
note in a walked folder of 5,000 notes, the write the deferred reindex makes
after every save.

**Budget: 50 ms, the slowest call**

Rationale:

- The upsert used to read every path in `files` and every row in `links` to
  write one note's rows, so its cost grew with the folder (RC-14, EP-16). It
  now resolves against the names kept in memory and touches no other note's
  rows, so its cost is the note's own. Measured 2026-10-13 on an M-series
  laptop, release: median and slowest both under 1 ms.

### Re-baselining

The budget is the save budget: it does not move with the folder. A failure
here is a write that reads the folder again.

---

## writ-storage: a rename with its links at 5,000 notes

**Test file:** `crates/writ-storage/tests/perf_budget.rs` (`a_rename_with_its_links_at_5000_notes_stays_under_budget`)

**What is measured:** the storage calls a rename with its links makes, in the
order the command makes them, for a note 22 others link to in a walked folder
of 5,000: the linking notes and every name a link could reach, read from the
index; the walk for files of the same name the index does not hold
(`note_ops::files_named`); the move; and, after the rewrites, the index brought
in line (the old path forgotten, the new one and every rewritten note
indexed). The rewrites run but are not timed: each is a guarded save, held to
the save round trip budget above. The command's own work around these calls is
not timed either.

**Budget: 300 ms** (RC-47). Measured 2026-10-13, release, at a load average
near 40: 102 to 150 ms over three runs, most of it the walk for same-named
files; the 22 rewrites took 321 to 429 ms besides.

Rationale: this is the part of a rename that grows with the folder. The walk
reads every entry of it, and the index writes re-resolve only the links that
name the note (ADR-036 section 7). A write that read every name again, or a
resolution that scanned every note per link, would cost the folder once per
linking note and breach the budget.

### Re-baselining

Raise the budget only for a change that legitimately adds work per rename,
and record the new measurement here. A rename that stops walking the whole
folder lowers it.

---

## writ-storage: notes index walk (5,000 notes)

**Test file:** `crates/writ-storage/tests/perf_budget.rs` (`index_5000_notes_reconcile_stays_under_budget`)

**What is measured:** one `NotesIndexStore::reconcile` over a fresh index and
a folder of 5,000 generated notes in fifty subfolders.

**Budget: 30 s** (`RECONCILE_BUDGET_MS`). Measured 2026-10-13, release: 2,489 ms.

Rationale: the walk runs on its own thread and holds the writer for one file
at a time, so its length is not the user's wait (the next budget is). The
budget catches a walk that becomes quadratic.

---

## writ-storage: commands during a walk

**Test file:** `crates/writ-storage/tests/perf_budget.rs` (`index_backed_commands_answer_within_budget_while_a_walk_holds_the_index`)

**What is measured:** while a walk of 5,000 notes runs on another thread, the
test issues, in turn and for as long as the walk lasts, a content search
keystroke, a quick open keystroke (which `[[` completion also reads), the
backlink list, a link's resolution, the folder graph and Cmd+N's writes for a
new note: its file, its row through the app's own connection, and the note in
the index. Every command is issued after the walk has started.

**Budget: 150 ms for the slowest command, 50 ms at p95, at least 20 commands
issued during the walk; for the folder graph, 50 ms at the median of at least
3 graphs drawn during the walk** (ADR-028 §7).

Rationale:

- The slowest command is the measure. One command that waits for the whole
  walk is the freeze, and a percentile over a few hundred samples discards
  exactly that command: the p99 this budget measured before 2026-10-13 read
  2 ms on the build where the first command issued during the walk took
  6,976 ms.
- The graph reads every note in the folder through a read connection, so its
  time is the folder's, not the walk's, and it never waits for the writer. It
  is held to the graph budget below, at its median, rather than to the
  ceiling: on a busy machine its own cost reached 96 to 170 ms with no walk
  running, which would decide the ceiling whatever the walk did.
- Cmd+N is timed as all of its storage writes. Its row goes through the
  app's own connection and meets the walk at SQLite's write lock: with
  SQLite's own busy handler, which waits up to 100 ms between tries, that row
  took 75 to 295 ms in three runs while the walk went on through up to 293
  more entries, and the command failed this budget at 321 ms. With the app's
  connections to `writ.db` retrying each millisecond (ADR-036 §7) it took 11
  to 19 ms.
- Measured 2026-10-13, release, after the writer went to waiting writes ahead
  of the walk and the app's connections retry each millisecond (ADR-036 §7),
  on a machine at a load average of 28 to 35, three runs: 710 to 730 commands
  besides the graph during each walk, p95 16 ms, slowest 29 to 36 ms (Cmd+N:
  its file 9 to 17 ms, its row 11 to 19 ms, the index 0 to 2 ms); the graph's
  median 2 ms and its slowest 13 to 15 ms.
- The minimum counts are what make a pass mean something: a run in which no
  command overlapped the walk has measured nothing.
- The budget is for a machine at rest. With other builds saturating the one
  these were measured on (load averages up to 72), the same code read 53 to
  151 ms at the slowest, readers included, which never wait for the writer.

---

## writ-storage: graph over 5,000 notes

**Test file:** `crates/writ-storage/tests/perf_budget.rs` (`graph_over_5000_notes_stays_under_the_query_budget`)

**What is measured:** `NotesIndexStore::graph` over a walked folder of 5,000
notes, nine times. **Budget: 50 ms at the median**, the keystroke budget
(ADR-036 §3), the measure the search budgets take: one read that lost the
processor to another program does not decide it. Measured 2026-10-13,
release: 16 and 21 ms at load averages of 21 to 35; a third run, on a machine
busy enough that the 5,000-note walk took 14.8 s rather than 3.7 to 5.1 s,
read 303 ms.

---

## writ-storage: vault scale (10,000 notes, 200,000 links)

**Test file:** `crates/writ-storage/tests/perf_budget.rs` (`vault_of_10000_notes_and_200000_links_stays_under_budget`).
**Bench:** `crates/writ-storage/benches/storage.rs`, group
`vault_10k_notes_200k_links` (`cargo bench -p writ-storage --bench storage -- vault`).

**What is measured:** a folder of 10,000 notes in a hundred subfolders, each
writing twenty links, five of them to one of the hundred notes most linked to
(about five hundred links each), walked into a fresh index. Then:

| Measure | Budget | Measured 2026-10-13, release |
|---|---|---|
| The walk from an empty index | 120 s | 28.7 s |
| Migration 044 over the 200,000 stored links | 10 s | 726 ms |
| A save of a linked note (slowest of 21) | 50 ms | 5 ms |
| Cmd+N, a new note (slowest of 21) | 50 ms | 4 ms |
| A note arriving and leaving under a name 500 links use (slowest of 21) | 50 ms | 13 ms |
| Backlinks of a much-linked note (slowest of 21) | 50 ms | 6 ms |
| A link resolved (slowest of 21) | 50 ms | 0 ms |
| Quick open keystroke (slowest of 21) | 50 ms | 4 ms |
| Content search keystroke (slowest of 21) | 50 ms | 2 ms |

Rationale:

- Every save, every new note and every keystroke is held to the 50 ms
  keystroke budget whatever the folder holds: the names are in memory, a link
  row carries the name it names (`links.name_key`, migration 044), and the
  backlink list and search read through indexes rather than the whole table
  (EP-16, RC-34).
- The names are read again only after another connection adds, removes or
  re-keys a note. The test rolls migration 044 back and runs it again through
  another connection before it times the first save, which reads no names.
- The migration runs once, at the first launch after the upgrade, inside the
  migration transaction. It reads no note.

### Re-baselining

The write and read budgets are the keystroke budget and do not move. Raise
the walk or migration budgets only for a change that legitimately adds work per
note or per link, and record the new measurement here.

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
