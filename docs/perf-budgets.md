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
release or after a change that touches a transform, the FTS layer, or buffer
I/O.

`scripts/perf-editor.mjs` is the browser half: it builds the real app over the
demo backend and measures it in headless Chromium, per file tier. The perf gate
runs it, and so does CI, on the Linux leg of every pull request. It reads the
same variable; see [editor: responsiveness per file tier](#editor-responsiveness-per-file-tier-headless-chromium).

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

## frontend: Find on large documents

**Test file:** `src/__tests__/editor/find-perf.test.ts`

**What is measured:** in an editor carrying the extensions `EditorInstance`
gives it, the longest main-thread task from one keystroke in the find field
(the query applied as the field's debounce applies it, counting every slice
the count is then read in), the same for one edit made with the overlay open,
and the time from each to the final count, for:

- a word, on generated prose of 4.9 MiB and 5.1 MiB, either side of the Normal
  tier's limit;
- regular expressions that cross lines, which the library's cursor reads in
  one pass over the whole document: `\s+zebrafish` on 4.9 MiB, and
  `\s+qxjzv` and `[^ ]qxjzv|zebrafish\s+qq` on 20 MiB, since V8 reads 5 MiB of
  these in under a frame.

On one generated 5 MiB line of JSON carrying accented words, a word
(`résumé`) and a pattern (`na.ve`) are read in 6 ms slices; the longest slice
and one edit, which must read only around itself, are held to the task budget.

Each count is checked against a plain count of the query in the text.

**Budgets: 16 ms per task (one frame at 60 Hz); 250 ms to the final count**

Rationale:

- Before the count ran in slices, one keystroke in the find field held the
  main thread for 547 ms on the 4.9 MiB file (editor-perf audit, 2026-10-03,
  Chromium); under vitest the same keystroke measured 989 ms. A slice stops
  after 6 ms, and neither a slice nor the keystroke itself may take a frame.
- Before patterns that cross lines and lines over 64 KiB were read in windows,
  the 20 MiB patterns held the main thread for 230 ms and 75 ms on each
  keystroke and each edit, and the 5 MiB line for 19.5 ms in one slice, an
  edit reading the whole line again (under vitest, 2026-10-13).
- Node runs V8, as Chromium does. Keystroke to paint in a browser is the
  editor gate's measure, below.
- Measured (eight runs under vitest, Node 26.4.0, Apple M1 Pro, 2026-10-13,
  load average 6.3 to 8.0 at the start and end of each). Seven ran within the
  budgets: the longest task 11.1 ms on a keystroke and 2.6 ms on an edit, the
  final count at most 129 ms (`\s+qxjzv` on 20 MiB); on the 5 MiB line the
  longest slice 6.2 ms, the whole read 22 ms, an edit 2.4 ms. The eighth ran
  slower throughout (the same 20 MiB count 167 ms) and read the 5 MiB line in
  74 ms with one slice of 27.5 ms, over the task budget.

The assertions are skipped unless `WRIT_PERF_GATE=1`. By hand:
`WRIT_PERF_GATE=1 pnpm exec vitest run src/__tests__/editor/find-perf.test.ts --disable-console-intercept`.

### Re-baselining

The task budget is the frame and does not move. The time to the final count
moves only with a deliberate change to the slice length, the size counted in
one go (`SLICE_MS` and `SYNC_SCAN_MAX_CHARS` in
`src/editor/search/find-matches.ts`) or the windows (`DEFAULT_SCAN_LIMITS` in
`src/editor/search/scanner.ts`); record the new measurement here.

---

## editor: responsiveness per file tier (headless Chromium)

**Gate:** `scripts/perf-editor.mjs`. The budgets live in
`scripts/perf-editor/budgets.mjs`; `scripts/perf-editor/__tests__/budgets.test.ts`
holds them to the table below.

**How it runs:** the gate builds `scripts/perf-editor/app` with the app's own
Vite config: the real `App` over the demo's in-memory backend, which answers
opens of generated files the way the Rust commands do (the tier from the size
and the binary sniff, the size question from 50 MiB to 500 MiB, the raw bytes
on a read; `demo/backend/classify.ts` mirrors `writ_core::file_ops`). Every
command's arguments go through Tauri's JSON serialisation at call time, so the
page pays the IPC send cost. Each scenario opens its file in a fresh headless
Chromium page at 1280x800, device scale 2, and drives it with key events. The
Rust side of an open (reads, hashes, the guarded write) is not in these
numbers.

```
WRIT_PERF_GATE=1 node scripts/perf-editor.mjs                    # every scenario but prose-300MiB
WRIT_PERF_GATE=1 node scripts/perf-editor.mjs --only prose-50MiB
WRIT_PERF_GATE=1 node scripts/perf-editor.mjs --large            # adds prose-300MiB
```

Chromium comes from `playwright-core`, a pinned dev dependency: install the
browser once per machine with `pnpm exec playwright-core install chromium`;
CI installs it in the job. `WRIT_PLAYWRIGHT_CORE` (the path to another copy's
`index.mjs`) and `WRIT_PERF_CHROMIUM` (a browser executable) override them.
The results, with the machine and the load average at the start and end of
every scenario, are written as JSON (`--out <file>`, by default to the system
temporary directory). Exit 1 means a budget was missed or a check failed; 2,
that the gate could not measure.

**What is measured** in each scenario:

| Metric | From and to |
|---|---|
| First paint | The open (in the confirm tier, Open in the size question) to the first frame after the editor shows the file |
| JS heap growth | The heap before the open to the heap after it, each after two forced collections |
| Jump to end | Cmd or Ctrl+End to the first frame that shows the end of the file |
| Typing p50, p95, max | Keydown to the end of the next frame, for 30 characters typed at the end 110 ms apart |
| Longest task after typing | The longest main-thread task in the 3.5 s after typing stops (the hash, the autosave) |
| Find keystroke p50, max | Keydown to the end of the next frame, typing in the find field a word that occurs about once per 100 KB and then one that occurs nowhere, 150 ms apart |
| Find to final count | The last keystroke in the find field to the first frame whose overlay shows the final count |
| Longest task typing in Find | The longest main-thread task from the first keystroke in the find field to the final count |
| Typing with Find open p50, max | Keydown to the end of the next frame, typing at the end with the overlay open on the first word |
| Longest task typing with Find open | The longest main-thread task from the first of those keystrokes to 3.5 s after the last, the hash and the autosave included |

A long task is one the browser reports, so over 50 ms; none is 0. Each
scenario also checks that the file opens in the mode its tier gives it, that
the size question is asked for the confirm tier and only there, that the
jump reaches the end, and that the count equals the word's occurrences in the
file before and after the edit. A page error fails the scenario.

**Scenarios** (fixed seeds, `src/__tests__/editor/fixtures/editor-corpus.ts`):

| Scenario | File | Bytes | Tier | Opens as |
|---|---|---:|---|---|
| `prose-1MiB` | prose | 1,048,576 | Normal | Normal |
| `prose-4.9MiB` | prose | 5,138,022 | Normal | Normal |
| `md-4.9MiB` | Markdown, a heading every 10,480 bytes, shown Inline | 5,138,022 | Normal | Normal |
| `json-1line-5MiB` | JSON on one line | 5,242,880 | Normal | LongLines |
| `prose-5.1MiB` | prose | 5,347,738 | LargeFile | LargeFile |
| `log-200k` | a log of 200,000 lines | 22,442,145 | LargeFile | LargeFile |
| `log-450k` | a log of 450,000 lines, the 50 MB log of the acceptance check | 50,494,988 | LargeFile | LargeFile |
| `prose-50MiB` | prose | 52,428,800 | LargeFile | LargeFile |
| `prose-60MiB` | prose, opened through the size question | 62,914,560 | LargeFileConfirm | LargeFile |
| `prose-300MiB` | prose, opened through the size question; `--large` only | 314,572,800 | LargeFileConfirm | LargeFile |

**Budgets**

<!-- editor-perf-budgets:start -->
| Scenario | Tier | First paint (ms) | JS heap growth (MiB) | Jump to end (ms) | Typing p50 (ms) | Typing p95 (ms) | Typing max (ms) | Longest task after typing (ms) | Find keystroke p50 (ms) | Find keystroke max (ms) | Find to final count (ms) | Longest task typing in Find (ms) | Typing with Find open p50 (ms) | Typing with Find open max (ms) | Longest task typing with Find open (ms) |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `prose-1MiB` | Normal | 80 | 4 | 65 | 40 | 60 | 100 | 100 | 35 | 100 | 475 | 100 | 50 | 100 | 100 |
| `prose-4.9MiB` | Normal | 145 | 11 | 75 | 35 | 55 | 100 | 100 | 35 | 100 | 465 | 100 | 40 | 100 | 100 |
| `md-4.9MiB` | Normal | 235 | 15 | 70 | 100 | 160 | 305 | 305 | 35 | 100 | 465 | 100 | 110 | 305 | 305 |
| `json-1line-5MiB` | Normal | 260 | 4 | 60 | 70 | 85 | 110 | 100 | 35 | 100 | 465 | 100 | 120 | 150 | 100 |
| `prose-5.1MiB` | LargeFile | 120 | 11 | 55 | 35 | 55 | 100 | 100 | 35 | 100 | 465 | 100 | 55 | 100 | 100 |
| `log-200k` | LargeFile | 305 | 41 | 55 | 40 | 60 | 210 | 210 | 35 | 100 | 465 | 100 | 45 | 240 | 240 |
| `log-450k` | LargeFile | 655 | 91 | 60 | 40 | 65 | 490 | 490 | 40 | 100 | 635 | 100 | 45 | 580 | 580 |
| `prose-50MiB` | LargeFile | 980 | 101 | 70 | 35 | 60 | 530 | 530 | 40 | 100 | 950 | 100 | 35 | 630 | 630 |
| `prose-60MiB` | LargeFileConfirm | 1195 | 121 | 60 | 35 | 55 | 800 | 800 | 45 | 100 | 1150 | 100 | 45 | 1170 | 1155 |
| `prose-300MiB` | LargeFileConfirm | 6515 | 605 | 75 | 35 | 60 | 4625 | 4625 | 40 | 100 | 4345 | 100 | 40 | 4625 | 4625 |
<!-- editor-perf-budgets:end -->

**How they were set:** three runs of each scenario, each started while the
one-minute load average was under 20 and taken again when it ended at 20 or
more. A time budget is three times the worst of the three runs and a heap
budget one and a half times, at least 4 MiB. A keystroke may take at least
34 ms (two frames at 60 Hz) and a longest task at least 100 ms. Typing with
Find open is allowed at least what typing is, and a keystroke's max at least
the longest task of its window, since a keystroke typed while Writ hashes the
document (150 ms after the last edit, 150 to 400 ms of work at 50 to 60 MiB)
waits for it. No budget is tighter than the editor-perf audit's figure for the
same file (2026-10-03, Chrome for Testing 153, Apple M1 Pro); `log-450k` and
`prose-60MiB` are held to the 50 MiB figures. The Find budgets have no audit
floor: the audit measured the count this gate replaced, which held the main
thread for 547 ms per keystroke at 4.9 MiB and 4,003 to 6,061 ms at 50 MiB, and
whose refresh on an edit put a 4.0 s task in the 50 MiB file's typing with
Find open. Times are rounded up to 5 ms and heap to 1 MiB. The headroom is for
CI's Linux runners, which are slower than the machine the runs were taken on.

**Measured** (worst of three runs; Chromium 153.0.8010.12 headless, Apple M1 Pro with 16 GiB, macOS 26.5, 2026-10-13; the one-minute load average at the start and end of each run beside every row):

| Scenario | Load average (start to end, per run) | First paint (ms) | JS heap growth (MiB) | Jump to end (ms) | Typing p50 (ms) | Typing p95 (ms) | Typing max (ms) | Longest task after typing (ms) | Find keystroke p50 (ms) | Find keystroke max (ms) | Find to final count (ms) | Longest task typing in Find (ms) | Typing with Find open p50 (ms) | Typing with Find open max (ms) | Longest task typing with Find open (ms) |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `prose-1MiB` | 6.6-5.5, 11.0-8.9, 6.6-6.1 | 26 | 1.5 | 21 | 12 | 19 | 21 | 0 | 2 | 11 | 158 | 0 | 16 | 20 | 0 |
| `prose-4.9MiB` | 5.5-5.1, 8.6-6.8, 5.9-5.0 | 47 | 6.9 | 24 | 11 | 18 | 19 | 0 | 9 | 20 | 154 | 0 | 13 | 17 | 0 |
| `md-4.9MiB` | 5.2-4.4, 6.8-5.5, 5.0-5.3 | 77 | 9.4 | 23 | 32 | 52 | 77 | 101 | 11 | 20 | 154 | 0 | 36 | 39 | 0 |
| `json-1line-5MiB` | 4.2-4.7, 5.2-5.1, 5.0-5.0 | 87 | 0.2 | 19 | 23 | 28 | 35 | 0 | 2 | 13 | 154 | 0 | 40 | 50 | 0 |
| `prose-5.1MiB` | 4.8-4.5, 5.0-4.0, 5.0-5.1 | 40 | 7.1 | 17 | 9 | 18 | 18 | 0 | 7 | 16 | 154 | 0 | 17 | 19 | 0 |
| `log-200k` | 4.2-5.5, 4.3-4.1, 5.1-5.4 | 101 | 26.9 | 18 | 13 | 19 | 21 | 70 | 7 | 16 | 154 | 0 | 15 | 20 | 79 |
| `log-450k` | 3.5-3.3, 4.0-3.6, 5.2-5.0 | 217 | 60.0 | 19 | 13 | 21 | 21 | 163 | 13 | 23 | 211 | 0 | 14 | 18 | 193 |
| `prose-50MiB` | 5.6-5.4, 3.9-5.0, 5.0-4.6 | 326 | 66.9 | 22 | 11 | 19 | 23 | 176 | 13 | 23 | 316 | 0 | 11 | 19 | 210 |
| `prose-60MiB` | 5.4-5.3, 5.4-6.6, 4.4-3.8 | 398 | 80.2 | 20 | 11 | 17 | 18 | 266 | 13 | 24 | 383 | 0 | 14 | 390 | 385 |
| `prose-300MiB` | 6.2-8.9, 6.7-6.9, 3.7-3.6 | 2170 | 402.9 | 24 | 11 | 19 | 19 | 1541 | 13 | 23 | 1447 | 0 | 13 | 19 | 1533 |

The toggle's frontend half is measured on every run too; its numbers and
budget are in [toggle-latency-baseline.md](perf/toggle-latency-baseline.md).

### WKWebView, once per release

macOS Writ runs in WKWebView (WebKit and JavaScriptCore), which the gate does
not drive. Once per release, on the installed build from the draft release:

1. `node scripts/perf-editor.mjs --write-files ~/Desktop/writ-perf` writes the
   scenarios' files (Node 22.18 or later).
2. Open `prose-4.9MiB.txt`, `notes-4.9MiB.md`, `prose-50MiB.txt` and
   `prose-60MiB.txt` from that folder, the last through Open in the size
   question, with a 60 fps screen recording running.
3. In each: Cmd+End, type a line at the end, Cmd+F and type `zebrafish`, then
   type at the end of the file with Find open.
4. From the recording, note the time from the open (or Open) to the text, the
   time from the last letter of `zebrafish` to the final count, the count, and
   any frame where typing stalled.

Record the results in this section with the version and date, newest first.
A result past the Chromium budget for the same file is a finding for that
release.

### Re-baselining

Re-measure with the procedure above (three quiet runs, load average under 20,
recorded beside the numbers) when a change moves a number on purpose, and
update `scripts/perf-editor/budgets.mjs` and this section together. A budget
other than a Find budget never drops below the audit figure for its file.

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

## writ-storage: FTS prefix-search latency

**Test file:** `crates/writ-storage/tests/perf_budget.rs` (`fts_prefix_search_budget`)

**What is measured:** median wall time (9-sample) of `BufferStore::search`
over the same 500-buffer corpus, using the quoted prefix queries the live
search-as-you-type path actually issues (`writ_core::search::to_prefix_match`,
e.g. `"rus"*`). Each query is asserted to match at least one row before
timing, so a silently-broken index cannot pass by matching nothing.

**Budget: 50 ms median per prefix query over 500 buffers**

Rationale:

- The prefix index (`prefix='2 3 4'`, migration 030) turns a 2–4 character
  prefix term from a full term-list scan into a seek. The 2026-05-22 audit
  measured the *unindexed* scan at 14 ms median over 10k buffers; with the
  index the same query is sub-millisecond. The 50 ms budget is tighter than
  the exact-match `FTS_BUDGET_MS` (200 ms) on purpose: it is the regression
  guard that fails if the prefix index is dropped, the migration is reordered,
  or the prefix query stops resolving. At 500 buffers a correctly indexed
  prefix query sits far under 50 ms, while a structural regression is visible
  well below the bound.
- The end-to-end 14 ms → sub-millisecond win is at the 10k scale of the audit
  bench; this gate proves the prefix path stays correct and fast at the
  realistic 500-buffer corpus rather than re-measuring the audit's 10k number
  on every CI run.

### Re-baselining

Raise `FTS_PREFIX_BUDGET_MS` only if the prefix configuration or tokenizer
changes in a way that legitimately raises latency, and document the change.

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
