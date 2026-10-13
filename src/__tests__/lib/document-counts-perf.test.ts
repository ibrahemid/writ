import { describe, it, expect } from "vitest";
import { Text } from "@codemirror/state";
import { TextCounter, type CountProgress } from "../../editor/document-counts";
import { COUNT_SLICE_MS } from "../../stores/window/counts-store";
import { generateCorpusText } from "../editor/fixtures/editor-corpus";

// What this measures: how long one slice of the status bar's count holds the
// main thread, how long a recount after a keystroke takes, and how much the
// heap grows while a document is counted, on the gate's 50 MiB prose and on a
// 300 MiB document. Node runs V8, as Chromium does; the counts on screen in a
// webview are the editor perf gate's `prose-4.9MiB-counts` and
// `log-300MiB-counts` (scripts/perf-editor.mjs).
const MiB = 1024 * 1024;
// One frame at 60 Hz.
const TASK_BUDGET_MS = 16;
// Far below one copy of the 50 MiB document; the whole-text count this
// replaced allocated 533 MiB for its split and 932 MiB for its spread.
const HEAP_GROWTH_BUDGET_MiB = 16;
// Each budget holds the fastest of this many runs: other work on the machine
// can slow a run down, never speed one up.
const RUNS = 3;

const gated = !process.env.WRIT_PERF_GATE;

interface CountRun {
  progress: CountProgress;
  longestSliceMs: number;
  heapGrowthMiB: number;
}

function countInSlices(doc: Text, counter = new TextCounter()): CountRun {
  const before = process.memoryUsage().heapUsed;
  let longestSliceMs = 0;
  let heapGrowth = 0;
  for (;;) {
    const started = performance.now();
    const progress = counter.count(doc, () => performance.now() - started >= COUNT_SLICE_MS);
    longestSliceMs = Math.max(longestSliceMs, performance.now() - started);
    heapGrowth = Math.max(heapGrowth, process.memoryUsage().heapUsed - before);
    if (progress.done) return { progress, longestSliceMs, heapGrowthMiB: heapGrowth / MiB };
  }
}

function fastest(values: number[]): number {
  return Math.min(...values);
}

describe("counting a 50 MiB document", () => {
  it.skipIf(gated)(
    "holds the main thread under a frame per slice, allocates no copy of the document, and recounts a keystroke within a frame",
    () => {
      const text = generateCorpusText({ name: "prose-50MiB.txt", kind: "prose", size: 50 * MiB });
      const lines = text.split("\n");
      const trimmed = text.trim();
      const words = trimmed === "" ? 0 : trimmed.split(/\s+/).length;
      const characters = [...text].length;

      const slices: number[] = [];
      const growth: number[] = [];
      const recounts: number[] = [];
      for (let run = 0; run < RUNS; run++) {
        const doc = Text.of(lines);
        const counter = new TextCounter();
        const counted = countInSlices(doc, counter);
        expect(counted.progress).toMatchObject({ done: true, words, characters });
        slices.push(counted.longestSliceMs);
        growth.push(counted.heapGrowthMiB);

        const at = doc.line(Math.floor(doc.lines / 2)).from;
        const edited = doc.replace(at, at, Text.of(["typed "]));
        const started = performance.now();
        const again = counter.count(edited, () => false);
        recounts.push(performance.now() - started);
        expect(again).toMatchObject({ done: true, words: words + 1, characters: characters + 6 });
      }

      console.log(
        `50 MiB count: longest slice ${slices.map((ms) => ms.toFixed(1)).join(", ")} ms; heap growth ${growth.map((mib) => mib.toFixed(1)).join(", ")} MiB; recount ${recounts.map((ms) => ms.toFixed(2)).join(", ")} ms`,
      );
      expect(fastest(slices)).toBeLessThan(TASK_BUDGET_MS);
      expect(fastest(growth)).toBeLessThan(HEAP_GROWTH_BUDGET_MiB);
      expect(fastest(recounts)).toBeLessThan(TASK_BUDGET_MS);
    },
    120_000,
  );
});

describe("counting a 300 MiB document", () => {
  it.skipIf(gated)(
    "finishes with the exact counts, no slice over a frame",
    () => {
      const line = "a line of a log that repeats until the file is three hundred mebibytes";
      const count = Math.ceil((300 * MiB) / (line.length + 1));
      const doc = Text.of(new Array<string>(count).fill(line));
      expect(doc.length).toBeGreaterThan(300 * MiB - line.length);

      const slices: number[] = [];
      for (let run = 0; run < RUNS; run++) {
        const counted = countInSlices(run === 0 ? doc : Text.of(new Array<string>(count).fill(line)));
        expect(counted.progress).toMatchObject({ done: true, words: count * 14, characters: doc.length });
        slices.push(counted.longestSliceMs);
      }

      console.log(`300 MiB count: longest slice ${slices.map((ms) => ms.toFixed(1)).join(", ")} ms`);
      expect(fastest(slices)).toBeLessThan(TASK_BUDGET_MS);
    },
    300_000,
  );
});
