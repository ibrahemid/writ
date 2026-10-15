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

Every timed figure is the best of three runs of its operation: the query typed
again into an empty field, the edit made again, the line read again from its
start. Other work on the machine can slow a run but never speed one up, and a
slower Find slows all three.

Each count is checked against a plain count of the query in the text, on every
run. For a regular expression that crosses lines that is its matches over the
whole text, which Find counts while a match and its lookarounds stay within
4 KiB of where it starts; ADR-013 records that bound and where the count
differs from the matches `SearchCursor` steps through.

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
- Measured (five runs of the file under vitest, Node 26.4.0, Apple M1 Pro,
  2026-10-13, load average 8.4 to 8.8 at the start and end of each; the
  slowest of the five best-of-three figures): the longest task 6.3 ms on a
  keystroke and 0.9 ms on an edit, the final count at most 88 ms (`\s+qxjzv`
  on 20 MiB); on the 5 MiB line the longest slice 6.2 ms, the whole read
  21 ms, an edit 1.6 ms. The slowest single run took 7.7 ms on a keystroke and
  93 ms to the final count. Before figures were the best of three, one of
  eight runs at load 6.3 to 8.0 read the 5 MiB line with one 27.5 ms slice and
  failed the task budget.

The assertions are skipped unless `WRIT_PERF_GATE=1`. By hand:
`WRIT_PERF_GATE=1 pnpm exec vitest run src/__tests__/editor/find-perf.test.ts --disable-console-intercept`.

### Re-baselining

The task budget is the frame and does not move. The time to the final count
moves only with a deliberate change to the slice length, the size counted in
one go (`SLICE_MS` and `SYNC_SCAN_MAX_CHARS` in
`src/editor/search/find-matches.ts`) or the windows (`DEFAULT_SCAN_LIMITS` in
`src/editor/search/scanner.ts`); record the new measurement here.

---

## frontend: word and character counts on large documents

**Test file:** `src/__tests__/lib/document-counts-perf.test.ts`

**What is measured:** the status bar's count (`TextCounter`,
`src/editor/document-counts.ts`) run the way the counts store runs it, in 6 ms
slices: on the 50 MiB prose of `prose-50MiB`, the longest slice, how far the
heap grows while the document is counted, and the recount after one
keystroke; on a 300 MiB document of one repeated line, the longest slice. Each
count is checked against the document: at 50 MiB against `trim().split(/\s+/)`
and `[...text]`, at 300 MiB against the line count.

**Budgets: 16 ms per slice and per recount (one frame at 60 Hz); 16 MiB of
heap growth while 50 MiB is counted**

Rationale:

- Before the counts were read from the document's own strings, every publish
  of the text split all of it and spread it into code points: at 50 MiB the
  split took 706 to 845 ms and allocated 533 to 564 MiB, the spread 1,348 to
  1,761 ms and 932 to 1,294 MiB (three runs under Node 26.4.0, Apple M1 Pro,
  2026-10-13, load average 3.7 to 5.0). With the status-bar counts on, the
  editor-perf audit typed at p50 72 ms in the 4.9 MiB file and crashed the
  renderer opening 300 MiB.
- 16 MiB is under a third of the 50 MiB document, so a copy of the text, or of
  its words, cannot fit in it. What the count allocates is one cursor per leaf
  of the document's tree, collected as it goes.
- Measured (three runs, Node 26.4.0, Apple M1 Pro, 2026-10-13, load average
  3.1 at the start): the longest slice 6.0 to 6.1 ms at 50 MiB and 6.0 to
  9.3 ms at 300 MiB, heap growth 4.3 to 5.6 MiB, a recount 0.26 to 0.31 ms.

Each budget holds the fastest of three runs. The assertions are skipped unless
`WRIT_PERF_GATE=1`. By hand:
`WRIT_PERF_GATE=1 pnpm exec vitest run src/__tests__/lib/document-counts-perf.test.ts --disable-console-intercept`.

### Re-baselining

The slice and recount budgets are the frame and do not move. The heap budget
moves only with a deliberate change to what the count keeps or makes per leaf
(`src/editor/document-counts.ts`); record the new measurement here.

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
WRIT_PERF_GATE=1 node scripts/perf-editor.mjs                    # every scenario but the two 300 MiB ones
WRIT_PERF_GATE=1 node scripts/perf-editor.mjs --only prose-50MiB
WRIT_PERF_GATE=1 node scripts/perf-editor.mjs --large            # adds prose-300MiB and log-300MiB-counts
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
| Longest task after typing | The longest main-thread task in the 3.5 s after typing stops (the autosave) |
| Find keystroke p50, max | Keydown to the end of the next frame, typing in the find field a word that occurs about once per 100 KB and then one that occurs nowhere, 150 ms apart |
| Find to final count | The last keystroke in the find field to the first frame whose overlay shows the final count |
| Longest task typing in Find | The longest main-thread task from the first keystroke in the find field to the final count |
| Typing with Find open p50, max | Keydown to the end of the next frame, typing at the end with the overlay open on the first word |
| Longest task typing with Find open | The longest main-thread task from the first of those keystrokes to 3.5 s after the last, the autosave included |

A long task is one the browser reports, so over 50 ms; none is 0. A window
counts a long task only when it started inside the window: the browser reports
a task after it ends, so one that began before a window opened can be reported
during it. Each scenario also checks that the file opens in the mode its tier
gives it, that the size question is asked for the confirm tier and only there,
that the jump reaches the end, and that the count equals the word's occurrences
in the file before and after the edit. With the counts on, it checks that the
status bar's counts finish, shown without `≈`, and up to 8 MiB that they equal
a count of the whole text. A page error fails the scenario.

**Scenarios** (fixed seeds, `src/__tests__/editor/fixtures/editor-corpus.ts`):

| Scenario | File | Bytes | Tier | Opens as |
|---|---|---:|---|---|
| `prose-1MiB` | prose | 1,048,576 | Normal | Normal |
| `prose-4.9MiB` | prose | 5,138,022 | Normal | Normal |
| `prose-4.9MiB-counts` | the same prose, with the word, character and token counts on in the status bar | 5,138,022 | Normal | Normal |
| `md-4.9MiB` | Markdown, a heading every 10,480 bytes, shown Inline | 5,138,022 | Normal | Normal |
| `md-blocks` | Markdown with 50 tables, 20 Mermaid diagrams, 20 display formulas and inline math between paragraphs, shown Inline with the blocks drawn; the scenario fails unless the first screen shows drawn tables, diagrams and formulas | 35,254 | Normal | Normal |
| `json-1line-5MiB` | JSON on one line | 5,242,880 | Normal | LongLines |
| `prose-5.1MiB` | prose | 5,347,738 | LargeFile | LargeFile |
| `log-200k` | a log of 200,000 lines | 22,442,145 | LargeFile | LargeFile |
| `log-450k` | a log of 450,000 lines, the 50 MB log of the acceptance check | 50,494,988 | LargeFile | LargeFile |
| `prose-50MiB` | prose | 52,428,800 | LargeFile | LargeFile |
| `prose-60MiB` | prose, opened through the size question | 62,914,560 | LargeFileConfirm | LargeFile |
| `prose-300MiB` | prose, opened through the size question; `--large` only | 314,572,800 | LargeFileConfirm | LargeFile |
| `log-300MiB-counts` | a log of 2,800,000 lines, the 300 MB log of the acceptance check, with the counts on, opened through the size question; `--large` only | 314,198,768 | LargeFileConfirm | LargeFile |

**Budgets**

<!-- editor-perf-budgets:start -->
| Scenario | Tier | First paint (ms) | JS heap growth (MiB) | Jump to end (ms) | Typing p50 (ms) | Typing p95 (ms) | Typing max (ms) | Longest task after typing (ms) | Find keystroke p50 (ms) | Find keystroke max (ms) | Find to final count (ms) | Longest task typing in Find (ms) | Typing with Find open p50 (ms) | Typing with Find open max (ms) | Longest task typing with Find open (ms) |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `prose-1MiB` | Normal | 80 | 4 | 65 | 40 | 60 | 100 | 100 | 35 | 100 | 475 | 100 | 50 | 100 | 100 |
| `prose-4.9MiB` | Normal | 145 | 11 | 75 | 35 | 55 | 100 | 100 | 35 | 100 | 465 | 100 | 40 | 100 | 100 |
| `prose-4.9MiB-counts` | Normal | 145 | 11 | 75 | 35 | 55 | 100 | 100 | 35 | 100 | 465 | 100 | 40 | 100 | 100 |
| `md-4.9MiB` | Normal | 235 | 15 | 70 | 100 | 160 | 305 | 305 | 35 | 100 | 465 | 100 | 110 | 305 | 305 |
| `md-blocks` | Normal | 95 | 6 | 165 | 35 | 55 | 100 | 100 | 35 | 100 | 510 | 100 | 45 | 100 | 100 |
| `json-1line-5MiB` | Normal | 260 | 4 | 60 | 70 | 85 | 110 | 100 | 35 | 100 | 465 | 100 | 120 | 150 | 100 |
| `prose-5.1MiB` | LargeFile | 120 | 11 | 55 | 35 | 55 | 100 | 100 | 35 | 100 | 465 | 100 | 55 | 100 | 100 |
| `log-200k` | LargeFile | 305 | 41 | 55 | 40 | 60 | 210 | 210 | 35 | 100 | 465 | 100 | 45 | 240 | 240 |
| `log-450k` | LargeFile | 655 | 91 | 60 | 40 | 65 | 490 | 490 | 40 | 100 | 635 | 100 | 45 | 580 | 580 |
| `prose-50MiB` | LargeFile | 980 | 101 | 70 | 35 | 60 | 530 | 530 | 40 | 100 | 950 | 100 | 35 | 630 | 630 |
| `prose-60MiB` | LargeFileConfirm | 1195 | 121 | 60 | 35 | 55 | 800 | 800 | 45 | 100 | 1150 | 100 | 45 | 1170 | 1155 |
| `prose-300MiB` | LargeFileConfirm | 6515 | 605 | 75 | 35 | 60 | 4625 | 4625 | 40 | 100 | 4345 | 100 | 40 | 4625 | 4625 |
| `log-300MiB-counts` | LargeFileConfirm | 2190 | 403 | 70 | 40 | 60 | 2335 | 2335 | 45 | 75 | 2485 | 100 | 40 | 2690 | 2690 |
<!-- editor-perf-budgets:end -->

**How they were set:** three runs of each scenario, each started while the
one-minute load average was under 20 and taken again when it ended at 20 or
more. A time budget is three times the worst of the three runs and a heap
budget one and a half times, at least 4 MiB. A keystroke may take at least
34 ms (two frames at 60 Hz) and a longest task at least 100 ms. Typing with
Find open is allowed at least what typing is, and a keystroke's max at least
the longest task of its window, since a keystroke typed during the autosave's
write of the whole document waits for it. Every budget outside Find is at
least the editor-perf audit's figure for the same file (its section 6,
2026-10-03, Chrome for Testing 153, Apple M1 Pro); `log-450k` and
`prose-60MiB` are held to the 50 MiB figures. With the counts on, the audit
typed at p50 72 ms in the 4.9 MiB file and crashed the renderer opening
300 MiB, so `prose-4.9MiB-counts` is held to the counts-off 4.9 MiB figures and
`log-300MiB-counts` to the 300 MiB ones. `prose-4.9MiB-counts` carries the
`prose-4.9MiB` budgets as they stand rather than three times its own runs:
with the counts on, typing is held to what it is with them off, and three
paired runs measured the two alike (typing p50 11 to 12 ms on, 13 to 15 ms
off; max 19 to 22 ms on, 20 to 22 ms off).
The Find budgets stand on this gate's own measurements, with no audit floor:
section 6 measured the count this gate replaced, which held the main thread
for 547 ms per keystroke at 4.9 MiB and 4,003 to 6,061 ms at 50 MiB, and whose
refresh on an edit put a 4.0 s task in the 50 MiB file's typing with Find
open. Held to those figures, the gate would pass a Find as slow as the one it
replaced. Two Find budgets sit below the audit's figures: Find keystroke p50 on
every file, and typing with Find open p50 on `prose-1MiB`, `prose-4.9MiB` and
`md-4.9MiB`. Times are rounded up to 5 ms and heap to 1 MiB. The headroom is for
CI's Linux runners, which are slower than the machine the runs were taken on.
`md-blocks` came after the audit, which measured no such file, so it has no
audit floor (`afterAudit` in `budgets.mjs`); its budgets follow the same rule
from three runs on 2026-10-15. Its numbers include drawing the blocks: KaTeX
typesets in a worker, and Mermaid lays each diagram out in a task of its own on
the page's thread, after typing pauses.

**Measured** (worst of three runs; Chromium 153.0.8010.12 headless, Apple M1 Pro with 16 GiB, macOS 26.5, 2026-10-13; the one-minute load average at the start and end of each run beside every row; taken before a window counted only the long tasks that started in it, which can only lower the three longest-task columns):

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
| `prose-4.9MiB-counts` | 4.7-3.7, 3.4-3.2, 3.5-3.0 | 47 | 2.0 | 22 | 12 | 21 | 22 | 0 | 1 | 20 | 154 | 0 | 17 | 19 | 0 |
| `log-300MiB-counts` | 4.5-4.7, 4.4-7.4, 6.8-6.0 | 590 | 78.5 | 23 | 12 | 19 | 20 | 778 | 14 | 24 | 827 | 0 | 12 | 19 | 896 |
| `md-blocks` | 5.8-6.0, 5.8-6.1, 6.1-5.7 | 30 | 3.7 | 55 | 11 | 18 | 19 | 0 | 2 | 16 | 170 | 0 | 14 | 18 | 0 |

The `prose-4.9MiB-counts` and `log-300MiB-counts` rows were taken later the
same day, with the window rule in place and the counts read from the
document's strings; the status bar showed the finished counts in every run
(995,271 words and 25,202,816 words).

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
4. With Settings, Editor, Word, character and token counts on, open
   `prose-4.9MiB.txt` and type a line at the end, then `server-300MiB.log`
   (written with `--large`) through Open in the size question, and watch the
   counts in the status bar go from `≈` to exact.
5. From the recording, note the time from the open (or Open) to the text, the
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

## writ-tauri: shell budget (main-thread time per IPC command)

**Test file:** `src-tauri/tests/shell_budget.rs`

**What is measured:** how long a command holds the thread that dispatched it.
Each command is sent through Tauri's mock runtime with `Webview::on_message`,
the call the webview's IPC handler makes on the main thread, so a command that
runs inline holds that thread for its whole body and one that is `async`
returns as soon as it is queued. The work (dispatch to answer) is reported
beside it.

**Budget: 16 ms (one frame at 60 Hz) per command**, held by the release build
under the perf gate. The always-on tests in the same file block each command's
work (the store, the note's queue, the render queue, the workspace index) and
fail when the dispatch does not return while it is blocked, so a command that
goes back to running inline fails `cargo test` as well.

Measured on 2026-10-14, release, on an M4 MacBook Air, before and after ADR-047.
Before, every listed command was synchronous, so the main thread was held for
the whole of its work.

| Command | Fixture | Main thread before | Main thread after | Work after |
|---|---|---|---|---|
| `open_file_confirmed` | 300 MB log | 311 ms | 0 ms | 313 ms |
| `read_buffer_content` | the same file | 315 ms | 0 ms | 300 ms |
| `save_buffer_content` | the same file, one byte changed | 1,107 ms | 0 ms | 960 ms |
| `save_buffer_content` (raw body) | the same file, one byte changed | not measured yet | | |
| `save_buffer_changes` | the same file, one byte changed as a change set | not measured yet | | |
| `preview_render` | 5 MB note, 54,359 wikilinks to 200 notes | 287 ms | 0 ms | 143 ms |
| `search_workspace_files` | 200,000 files, slowest of six keystrokes | 104 ms | 0 ms | 104 ms |
| `install_cli` | administrator password dialog | the whole dialog | 0 ms | the person |
| `mcp_connect_cards`, `mcp_connect_add`, `mcp_connect_open`, `mcp_server_test` | a client's own command, the login shell, `writ mcp` | not measured yet | 0 ms | the other program |
| `context_answer` | the selection of the file open in the window, up to 2 MB | async from its first version | 0 ms | not measured yet |
| `review_answer` | the person's answer to a program's held write | async from its first version | 0 ms | not measured yet |

Notes:

- `install_cli` is paced by the person at the dialog and is not measured
  headless; it runs on the blocking pool (`run_detached`), and the editor
  repainting while the dialog is up is a smoke row.
- The client-card commands (`commands/mcp_connect.rs`, ADR-031 rule 2.4) are
  paced by another program: the login shell that reports the `PATH` (stopped
  after 5 s), a client's own `mcp add` (30 s), the opener and `writ mcp` (10 s).
  Each runs on the blocking pool and is not measured headless.
- `context_answer` (ADR-053) writes one answer file to the data folder. Its
  always-on test holds the buffer store, which the work reads the tab's file
  from.
- `review_answer` (ADR-053) writes one answer file to the data folder. Its
  always-on test holds the settings, which the work reads to confirm the
  program may still write.
- The search row's "before" is its work: the ranking code did not change, only
  where it runs.
- The render's work halved because a note's wikilinks are resolved in one
  lookup before it renders.
- `prompt_context_pack` (a file and the files it links to, up to 100, as one
  prompt) was `async` from its first version, so it has no "before". Its
  always-on test holds its work; its timed test packs 100 files of 40 KB.
- `list_workspace_dir` (one folder of the sidebar's tree) runs on the
  blocking pool since ADR-047. Its always-on test holds the open folder's
  root; its timed test lists one folder of 20,000 entries.
- `save_buffer_content` takes the text as a raw body with the note in the
  `writ-note-id` header (EP-13): nothing is serialised to JSON or parsed on
  either side. The large tiers send `save_buffer_changes` once one write has
  landed. Both rows are in the 300 MB timed test; the release numbers are
  taken by the next `scripts/perf-gate.sh` run.

### Re-baselining

Add a command to the budget by registering it in `mock_app` and adding an
always-on test that blocks its work and a timed one under `WRIT_PERF_GATE`.
Change `SHELL_BUDGET_MS` only with the reason here.

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

**Budget: 300 ms** (RC-47): twice the slowest of three release runs on
2026-10-13 at a one-minute load average of 21 to 27, rounded up to the next
50 ms. Those runs took 91, 123 and 133 ms, most of it the walk for same-named
files; the 22 rewrites took 288 to 751 ms besides.

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
| Migration 045 over the 200,000 stored links | 10 s | 726 ms |
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
  row carries the name it names (`links.name_key`, migration 045), and the
  backlink list and search read through indexes rather than the whole table
  (EP-16, RC-34).
- The names are read again only after another connection adds, removes or
  re-keys a note. The test rolls migration 045 back and runs it again through
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

## app: shipped binary size

**Checked by hand at the release preflight;** no test reads it.

**What is measured:** the size in bytes of the `writ-tauri` executable each
release ships, per platform, and of each bundle. The release profile unwinds on
panic and keeps line tables in separate symbol files (ADR-045); unwind tables
and landing pads are the part of that change that stays in the binary.

**Budget: each executable and each bundle at most 15% over the previous
release.** A release past it says why in its notes.

| Artifact | 0.6.0 (`panic = "abort"`, `strip = true`) | 0.7.0 |
|---|---|---|
| `writ-tauri`, macOS arm64 slice | 24,974,192 | at the 0.7.0 preflight |
| `writ-tauri`, macOS x86_64 slice | 27,847,680 | at the 0.7.0 preflight |
| `writ-tauri`, Linux x86_64 (deb) | 31,451,480 | at the 0.7.0 preflight |
| `writ-tauri.exe`, Windows x64 | not recorded | at the 0.7.0 preflight |
| `Writ_<v>_universal.dmg` | 29,677,412 | at the 0.7.0 preflight |
| `Writ_<v>_x64_en-US.msi` | 15,163,392 | at the 0.7.0 preflight |
| `Writ_<v>_amd64.deb` | 16,812,232 | at the 0.7.0 preflight |
| `Writ_<v>_amd64.AppImage` | 94,083,576 | at the 0.7.0 preflight |

Method:

- Bundles: `gh release view v<version> --json assets` lists each asset's size.
- macOS slices: unpack `Writ_<v>_universal.app.tar.gz` and read
  `lipo -detailed_info Writ.app/Contents/MacOS/writ-tauri`.
- Linux: `ar x Writ_<v>_amd64.deb`, then `tar -tvzf data.tar.gz | grep usr/bin/writ-tauri`.
- Windows: the `writ-tauri.exe` in the run's `build-windows-binaries` artifact.
- Before a tag, the same commands run on the preflight's local build
  (`scripts/release-preflight.sh`) give the number to compare.

A debug build does not stand in for these: the dev profile already unwinds and
keeps full debug info. For reference, `cargo build -p writ-tauri` on
aarch64-apple-darwin at the branch that switched the release profile gave a
130,914,872-byte `target/debug/writ-tauri` (`stat -f %z`).

### Re-baselining

Each release's row replaces the previous one once it ships. Raise the 15% only
for a change that adds a dependency or a bundled asset on purpose, and name it
here.

---

## app: watcher events per second

What a folder watcher may send the webview about the folder itself. Each
watcher spends an `EmissionBudget` (`writ-core/src/watcher/budget.rs`) per
500 ms debounce window: nine changes named, then one sweep that stands for the
rest, and at most one sweep per 2 s cooldown.

| Watcher | Budget | Gate |
|---|---|---|
| Notes folder (`NotesChanged`, `NotesSwept`) | 10 events per 500 ms window, 20 per second | `writ-core` budget tests |
| Workspace folder (`WorkspaceChanged`, `WorkspaceSwept`) | 10 events per 500 ms window, 20 per second | `a_workspace_burst_stays_within_the_budget` in `src-tauri/src/watcher/handler.rs`: 10,000 paths in one window give 10 events, the last a sweep |
| Workspace folder, an install into `.venv`, `venv`, `node_modules` and the other default-ignored folders | 0 events | `an_install_into_a_venv_inside_the_workspace_raises_no_event` in `src-tauri/tests/workspace_tests.rs` |

Telling an open tab its own file changed is outside the budget and bounded by
the number of open tabs, one message per tab per window.

### Re-baselining

The numbers are the constants in `budget.rs`; change them there and in this
table together.

---

## Fixture determinism guarantee

Both the transform fixture and the storage corpus are fully deterministic:
no random number generators, no time-seeded data, no OS entropy. A test
asserts fixture/corpus determinism explicitly. If you add randomness anywhere
in a fixture builder, add an accompanying assertion that two calls produce
identical output.
