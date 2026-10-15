import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { ensureSyntaxTree } from "@codemirror/language";
import {
  activeLineStarts,
  buildMarkdownDecorations,
} from "../../editor/markdown-typography";
import { collapsibleFences } from "../../editor/markdown-fences";
import { markdownBlockField, markdownBlocks, setBlockWindow } from "../../editor/markdown-blocks";
import { MARKDOWN_EXTENSIONS } from "../../editor/markdown-math";
import { buildMarkdownCorpus } from "./fixtures/markdown-corpus";
import { generateCorpusText } from "./fixtures/editor-corpus";

// What this measures: the wall time of one decoration build over one
// viewport, in Node. jsdom lays nothing out, so keystroke-to-paint cannot be
// measured here; end-to-end latency is a smoke row, not a gate number.
const BUILD_BUDGET_MS = 16;
// The fence scan runs over the window the view plugin publishes, on the same
// rebuild as the decorations.
const FENCE_SCAN_BUDGET_MS = 16;
// The block field's scan and widget build for one window, which runs on every
// edit and every scroll that moves the viewport.
const BLOCK_SCAN_BUDGET_MS = 16;
// Matches the convention in crates/writ-storage/tests/perf_budget.rs.
const MEDIAN_SAMPLES = 9;
const CORPUS_BYTES = 2 * 1024 * 1024;
const VIEWPORT_LINES = 60;
const PARSE_TIMEOUT_MS = 120_000;

const gated = !process.env.WRIT_PERF_GATE;

function median(samples: number[]): number {
  const sorted = [...samples].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

// The gate reads the measured median off the run, the way the Rust budgets
// print theirs.
function report(name: string, ms: number, budget: number): void {
  console.log(`${name}: median ${ms.toFixed(2)} ms (budget ${budget} ms)`);
}

describe("inline markdown decoration budget", () => {
  it.skipIf(gated)(
    "builds one viewport of a 2 MB document in under 16ms",
    () => {
      const corpus = buildMarkdownCorpus(CORPUS_BYTES);
      const state = EditorState.create({
        doc: corpus,
        extensions: [markdown({ base: markdownLanguage })],
      });
      const tree = ensureSyntaxTree(state, corpus.length, PARSE_TIMEOUT_MS);
      expect(tree?.length).toBe(corpus.length);
      const middle = state.doc.lineAt(Math.floor(corpus.length / 2));
      const visibleFrom = middle.from;
      const visibleTo = state.doc.line(
        Math.min(middle.number + VIEWPORT_LINES, state.doc.lines),
      ).to;

      const samples: number[] = [];
      for (let run = 0; run < MEDIAN_SAMPLES; run++) {
        const started = performance.now();
        const specs = buildMarkdownDecorations(
          (from, to, cb) => tree!.iterate({ from, to, enter: cb }),
          (pos) => state.doc.lineAt(pos),
          (from, to) => state.doc.sliceString(from, to),
          activeLineStarts(
            [{ from: visibleFrom, to: visibleFrom }],
            (pos) => state.doc.lineAt(pos),
            visibleFrom,
            visibleTo,
          ),
          visibleFrom,
          visibleTo,
        );
        samples.push(performance.now() - started);
        expect(specs.length).toBeGreaterThan(0);
      }
      const measured = median(samples);
      report("build one viewport", measured, BUILD_BUDGET_MS);
      expect(measured).toBeLessThan(BUILD_BUDGET_MS);
    },
    PARSE_TIMEOUT_MS,
  );

  it.skipIf(gated)(
    "a select-all on a 2 MB document still builds one viewport in under 16ms",
    () => {
      const corpus = buildMarkdownCorpus(CORPUS_BYTES);
      const state = EditorState.create({
        doc: corpus,
        extensions: [markdown({ base: markdownLanguage })],
      });
      const tree = ensureSyntaxTree(state, corpus.length, PARSE_TIMEOUT_MS);
      expect(tree?.length).toBe(corpus.length);
      const middle = state.doc.lineAt(Math.floor(corpus.length / 2));
      const visibleFrom = middle.from;
      const visibleTo = state.doc.line(
        Math.min(middle.number + VIEWPORT_LINES, state.doc.lines),
      ).to;

      const samples: number[] = [];
      for (let run = 0; run < MEDIAN_SAMPLES; run++) {
        const started = performance.now();
        buildMarkdownDecorations(
          (from, to, cb) => tree!.iterate({ from, to, enter: cb }),
          (pos) => state.doc.lineAt(pos),
          (from, to) => state.doc.sliceString(from, to),
          activeLineStarts(
            [{ from: 0, to: corpus.length }],
            (pos) => state.doc.lineAt(pos),
            visibleFrom,
            visibleTo,
          ),
          visibleFrom,
          visibleTo,
        );
        samples.push(performance.now() - started);
      }
      const measured = median(samples);
      report("build one viewport under a select-all", measured, BUILD_BUDGET_MS);
      expect(measured).toBeLessThan(BUILD_BUDGET_MS);
    },
    PARSE_TIMEOUT_MS,
  );

  it.skipIf(gated)(
    "scans one viewport of fenced blocks on a 2 MB document in under 16ms",
    () => {
      const corpus = buildMarkdownCorpus(CORPUS_BYTES);
      const state = EditorState.create({
        doc: corpus,
        extensions: [markdown({ base: markdownLanguage })],
      });
      // The whole tree, not the lazily parsed prefix syntaxTree() holds: the
      // window is what bounds the scan, and a short tree would hide that.
      const tree = ensureSyntaxTree(state, corpus.length, PARSE_TIMEOUT_MS);
      expect(tree?.length).toBe(corpus.length);
      const middle = state.doc.lineAt(Math.floor(corpus.length / 2));
      const visibleFrom = middle.from;
      const visibleTo = state.doc.line(
        Math.min(middle.number + VIEWPORT_LINES, state.doc.lines),
      ).to;

      const samples: number[] = [];
      let found = 0;
      for (let run = 0; run < MEDIAN_SAMPLES; run++) {
        const started = performance.now();
        found = collapsibleFences(
          (from, to, cb) => tree!.iterate({ from, to, enter: cb }),
          (pos) => state.doc.lineAt(pos),
          visibleFrom,
          visibleTo,
        ).length;
        samples.push(performance.now() - started);
      }
      const measured = median(samples);
      report(`scan ${found} fenced blocks in one viewport`, measured, FENCE_SCAN_BUDGET_MS);
      expect(found).toBeGreaterThan(0);
      expect(measured).toBeLessThan(FENCE_SCAN_BUDGET_MS);
    },
    PARSE_TIMEOUT_MS,
  );

  it.skipIf(gated)(
    "scans and builds the block widgets of one window of 50 tables and 20 diagrams in under 16ms",
    () => {
      const doc = generateCorpusText({
        name: "blocks.md",
        kind: "markdown-blocks",
        size: 0,
        tables: 50,
        diagrams: 20,
      });
      let state = EditorState.create({
        doc,
        extensions: [
          markdown({ base: markdownLanguage, extensions: MARKDOWN_EXTENSIONS }),
          markdownBlocks({
            renderMath: () => new Promise(() => {}),
            renderDiagram: () => new Promise(() => {}),
          }),
        ],
      });
      const tree = ensureSyntaxTree(state, doc.length, PARSE_TIMEOUT_MS);
      expect(tree?.length).toBe(doc.length);
      const middle = state.doc.lineAt(Math.floor(doc.length / 2));
      const windows = [0, 1].map((shift) => ({
        from: state.doc.line(middle.number + shift).from,
        to: state.doc.line(Math.min(middle.number + shift + VIEWPORT_LINES * 2, state.doc.lines)).to,
      }));

      const samples: number[] = [];
      let drawn = 0;
      for (let run = 0; run < MEDIAN_SAMPLES; run++) {
        const started = performance.now();
        state = state.update({ effects: setBlockWindow.of(windows[run % 2]) }).state;
        drawn = state.field(markdownBlockField).drawn.length;
        samples.push(performance.now() - started);
      }
      const measured = median(samples);
      report(`scan and build ${drawn} blocks in one window`, measured, BLOCK_SCAN_BUDGET_MS);
      expect(drawn).toBeGreaterThan(0);
      expect(measured).toBeLessThan(BLOCK_SCAN_BUDGET_MS);
    },
    PARSE_TIMEOUT_MS,
  );

  it.skipIf(gated)(
    "reads the whole 2 MB document only when the window says to",
    () => {
      // The cost the window exists to avoid, measured so the difference is on
      // the record rather than asserted.
      const corpus = buildMarkdownCorpus(CORPUS_BYTES);
      const state = EditorState.create({
        doc: corpus,
        extensions: [markdown({ base: markdownLanguage })],
      });
      const tree = ensureSyntaxTree(state, corpus.length, PARSE_TIMEOUT_MS);
      expect(tree?.length).toBe(corpus.length);
      const samples: number[] = [];
      let found = 0;
      for (let run = 0; run < MEDIAN_SAMPLES; run++) {
        const started = performance.now();
        found = collapsibleFences(
          (from, to, cb) => tree!.iterate({ from, to, enter: cb }),
          (pos) => state.doc.lineAt(pos),
          0,
          state.doc.length,
        ).length;
        samples.push(performance.now() - started);
      }
      report(`scan ${found} fenced blocks in the whole document`, median(samples), 0);
      expect(found).toBeGreaterThan(0);
    },
    PARSE_TIMEOUT_MS,
  );
});
