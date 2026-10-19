import { describe, it, expect } from "vitest";
import { createRoot } from "solid-js";
import { EditorView } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { search, SearchQuery } from "@codemirror/search";
import { findMatches } from "../../editor/search/find-matches";
import { MatchIndex } from "../../editor/search/match-index";
import { createQueryScanner } from "../../editor/search/scanner";
import { createFindController, createEditorSurface } from "../../stores/global/find-store";
import { generateCorpusText, RARE_WORD } from "./fixtures/editor-corpus";

// What this measures: how long Find holds the main thread for one keystroke in
// the find field and for one edit made while Find is open, on documents either
// side of the 5 MiB tier line, and how long the count takes to settle. The
// view carries the extensions EditorInstance gives it; the work a later slice
// does is timed through the slice scheduler. Node runs V8, as Chromium does;
// keystroke-to-paint in a webview is the editor perf gate's number
// (scripts/perf-editor.mjs), not this one.
const MiB = 1024 * 1024;
// One frame at 60 Hz: neither the keystroke nor any slice it starts may be longer.
const TASK_BUDGET_MS = 16;
// From the keystroke to the final count.
const SETTLE_BUDGET_MS = 250;
const SETTLE_TIMEOUT_MS = 30_000;
// Each budget holds the fastest of this many runs of its operation: other work
// on the machine can slow a run down, never speed one up.
const RUNS = 3;

const gated = !process.env.WRIT_PERF_GATE;

// A word, and patterns that cross lines (`\s`, `[^`), which the
// library's cursor reads in one pass over the whole document; V8 reads 5 MiB
// of these in under a frame, so they run on 20 MiB too.
const CASES = [
  { label: "4.9 MiB", size: Math.round(4.9 * MiB), query: RARE_WORD, regexp: false, insert: ` ${RARE_WORD}` },
  { label: "5.1 MiB", size: Math.round(5.1 * MiB), query: RARE_WORD, regexp: false, insert: ` ${RARE_WORD}` },
  { label: "4.9 MiB regexp", size: Math.round(4.9 * MiB), query: `\\s+${RARE_WORD}`, regexp: true, insert: ` ${RARE_WORD}` },
  { label: "20 MiB regexp", size: 20 * MiB, query: "\\s+qxjzv", regexp: true, insert: " qxjzv" },
  { label: "20 MiB negated class", size: 20 * MiB, query: `[^ ]qxjzv|${RARE_WORD}\\s+qq`, regexp: true, insert: "-qxjzv" },
];

/** Runs each slice in its own task and keeps how long it took. */
function timedSlices() {
  const durations: number[] = [];
  const schedule = (slice: () => void) => {
    const handle = setImmediate(() => {
      const started = performance.now();
      slice();
      durations.push(performance.now() - started);
    });
    return () => clearImmediate(handle);
  };
  return { durations, schedule };
}

function makeView(doc: string, schedule: (slice: () => void) => () => void): EditorView {
  const state = EditorState.create({ doc, extensions: [search({ top: true }), findMatches({ schedule })] });
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  return new EditorView({ state, parent });
}

function countReference(text: string, query: string, regexp: boolean): number {
  if (regexp) return [...text.matchAll(new RegExp(query, "gimu"))].length;
  const lower = text.toLowerCase();
  let count = 0;
  for (let at = lower.indexOf(query); at !== -1; at = lower.indexOf(query, at + query.length)) count++;
  return count;
}

type Find = ReturnType<typeof createFindController>;

/** Waits until the count is final and returns the time since `started`. */
async function settle(find: Find, started: number): Promise<number> {
  for (;;) {
    find.refresh();
    if (!find.matches().pending) return performance.now() - started;
    if (performance.now() - started > SETTLE_TIMEOUT_MS) throw new Error("the count never settled");
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
}

const best = (runs: readonly number[]): number => Math.min(...runs);

function report(name: string, runs: readonly number[], budget: number): void {
  const each = runs.map((ms) => ms.toFixed(2)).join(", ");
  console.log(`${name}: best ${best(runs).toFixed(2)} ms of ${each} (budget ${budget} ms)`);
}

describe("find on a large document", () => {
  for (const { label, size, query, regexp, insert } of CASES) {
    it.skipIf(gated)(
      `${label}: one find keystroke and one edit with Find open stay under a frame`,
      async () => {
        const text = generateCorpusText({ name: `prose-${label}.txt`, kind: "prose", size });
        const expected = countReference(text, query, regexp);
        const slices = timedSlices();
        const view = makeView(text, slices.schedule);
        let find!: Find;
        const dispose = createRoot((disposeRoot) => {
          find = createFindController(() => createEditorSurface(() => view));
          return disposeRoot;
        });
        try {
          find.open();
          if (regexp) find.toggleRegexp();

          const keyTasks: number[] = [];
          const keySettles: number[] = [];
          for (let run = 0; run < RUNS; run++) {
            find.setQueryText("");
            await settle(find, performance.now());
            slices.durations.length = 0;
            const keyStarted = performance.now();
            find.setQueryText(query);
            const keySync = performance.now() - keyStarted;
            keySettles.push(await settle(find, keyStarted));
            keyTasks.push(Math.max(keySync, ...slices.durations));
            expect(slices.durations.length).toBeGreaterThan(0);
            expect(find.matches().total).toBe(expected);
          }

          const editTasks: number[] = [];
          const editSettles: number[] = [];
          for (let run = 0; run < RUNS; run++) {
            slices.durations.length = 0;
            const editStarted = performance.now();
            view.dispatch({ changes: { from: view.state.doc.length, insert } });
            find.refresh();
            const editSync = performance.now() - editStarted;
            editSettles.push(await settle(find, editStarted));
            editTasks.push(Math.max(editSync, ...slices.durations));
            expect(find.matches().total).toBe(countReference(view.state.doc.toString(), query, regexp));
          }

          report(`${label} find keystroke longest task`, keyTasks, TASK_BUDGET_MS);
          report(`${label} find keystroke to final count`, keySettles, SETTLE_BUDGET_MS);
          report(`${label} edit with Find open longest task`, editTasks, TASK_BUDGET_MS);
          report(`${label} edit with Find open to final count`, editSettles, SETTLE_BUDGET_MS);
          expect(best(keyTasks)).toBeLessThan(TASK_BUDGET_MS);
          expect(best(keySettles)).toBeLessThan(SETTLE_BUDGET_MS);
          expect(best(editTasks)).toBeLessThan(TASK_BUDGET_MS);
          expect(best(editSettles)).toBeLessThan(SETTLE_BUDGET_MS);
        } finally {
          dispose();
          view.destroy();
        }
      },
      120_000,
    );
  }
});

describe("find on one 5 MiB line", () => {
  it.skipIf(gated)(
    "reads the line in slices under a frame, and an edit reads only around itself",
    () => {
      const state = EditorState.create({
        doc: generateCorpusText({ name: "one-line.json", kind: "single-line-json", size: Math.round(5 * MiB) }),
      });
      const editAt = Math.floor(state.doc.length / 2);
      for (const query of [new SearchQuery({ search: "résumé" }), new SearchQuery({ search: "na.ve", regexp: true })]) {
        const longestSlices: number[] = [];
        const reads: number[] = [];
        const edits: number[] = [];
        for (let run = 0; run < RUNS; run++) {
          const index = new MatchIndex(createQueryScanner(query)!);
          let longest = 0;
          const started = performance.now();
          while (!index.settled) {
            const slice = performance.now();
            index.advance(state, slice + 6);
            longest = Math.max(longest, performance.now() - slice);
          }
          reads.push(performance.now() - started);
          longestSlices.push(longest);
          const count = index.count;
          const editStarted = performance.now();
          const tr = state.update({ changes: { from: editAt, insert: " résumé naïve" } });
          index.applyChanges(tr.changes, tr.startState.doc, tr.state);
          edits.push(performance.now() - editStarted);
          expect(index.settled).toBe(true);
          expect(index.count).toBe(count + 1);
        }
        report(`one line ${query.search} longest slice`, longestSlices, TASK_BUDGET_MS);
        report(`one line ${query.search} read`, reads, SETTLE_BUDGET_MS);
        report(`one line ${query.search} edit`, edits, TASK_BUDGET_MS);
        expect(best(longestSlices)).toBeLessThan(TASK_BUDGET_MS);
        expect(best(edits)).toBeLessThan(TASK_BUDGET_MS);
      }
    },
    120_000,
  );
});

// The editor gate's find-rate budget (scripts/perf-editor/budgets.mjs), held
// here to the reading alone.
const RATE_BUDGET_MS_PER_MIB = 1.6;

describe("find on 20 MiB of prose", () => {
  it.skipIf(gated)(
    "reads a word's last keystroke within the find rate",
    () => {
      const size = 20 * MiB;
      const state = EditorState.create({ doc: generateCorpusText({ name: "prose-20MiB.txt", kind: "prose", size }) });
      const read = (search: string) => {
        const index = new MatchIndex(createQueryScanner(new SearchQuery({ search }))!);
        index.advance(state, Infinity);
        return index;
      };
      for (const word of [RARE_WORD, "qxjzv"]) {
        // Typing the rest of the word reads the document first.
        read(word.slice(0, -1));
        const rates: number[] = [];
        for (let run = 0; run < RUNS; run++) {
          const started = performance.now();
          read(word);
          rates.push((performance.now() - started) / (size / MiB));
        }
        report(`20 MiB prose ${word} ms per MiB`, rates, RATE_BUDGET_MS_PER_MIB);
        expect(best(rates)).toBeLessThan(RATE_BUDGET_MS_PER_MIB);
      }
    },
    120_000,
  );
});
