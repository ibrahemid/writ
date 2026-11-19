import { afterEach, describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { ensureSyntaxTree, syntaxHighlighting } from "@codemirror/language";
import { jotraHighlight } from "../../components/Editor/cm-theme";
import {
  NON_PROSE_HEIGHT_LINE_CLASSES,
  markdownInlineExtension,
} from "../../editor/markdown-typography";
import { MARKDOWN_EXTENSIONS } from "../../editor/markdown-math";
import type { BlockRenderer } from "../../editor/block-renderer";
import type { InlineImageDeps } from "../../editor/markdown-images";
import { generateCorpusText } from "./fixtures/editor-corpus";

// CodeMirror sets its default line height from the first rendered line of at
// most 20 printable ASCII characters whose children are all text
// (DocView.measureTextSize), and rebuilds every estimated height when that
// sample changes. A tab restored with `setState` takes a fresh sample, so a
// line sized away from prose must never be the sample.
const SAMPLE_MAX_LENGTH = 20;

// The perf gate's Markdown note with a few bytes changed per section: the
// renames inside its links and fences.
const SHIFTS: ReadonlyArray<readonly [string, string]> = [
  ["writ.example", "jotra.example"],
  ["~/Writ", "~/Jotra"],
  ['"writ"', '"jotra"'],
];
const NOTE_BYTES = Math.round(4.9 * 1024 * 1024);
const SECTION_BYTES = 10_480;
// The stretch around the middle of the note, where the gate's tab round trip
// leaves the view.
const WINDOW_BYTES = 96 * 1024;
const PARSE_TIMEOUT_MS = 30_000;

// Each block keeps a line short and bare enough to be sampled without its
// line-class opt-out.
const HAND_WRITTEN_BLOCKS: ReadonlyArray<readonly [string, string]> = [
  ["fenced code", "```py\nx = 1\n```"],
  ["indented code", "    x = 1"],
  ["table", "| a | b |\n|---|---|\n| 1 | 2 |\nbare row"],
  ["html", "<div>\nhtml\n</div>"],
  ["display math", "$$\nx^2\n$$"],
  ["heading", "# One\n## Two\n### Three\n#### Four\n##### Five\n###### Six"],
];

const NEVER_RENDERS: BlockRenderer = {
  renderMath: () => new Promise(() => {}),
  renderDiagram: () => new Promise(() => {}),
};

const NO_IMAGES: InlineImageDeps = {
  resolve: () => new Promise(() => {}),
  download: () => new Promise(() => {}),
  loadRemote: () => new Promise(() => {}),
  loadsRemoteImages: () => false,
};

interface SampleScan {
  nonProseLines: number;
  sampled: Map<number, string>;
}

function shiftedNoteMiddle(): string {
  let text = generateCorpusText({
    name: "notes-4.9MiB.md",
    kind: "markdown",
    size: NOTE_BYTES,
    sectionBytes: SECTION_BYTES,
  });
  for (const [from, to] of SHIFTS) text = text.replaceAll(from, to);
  const sectionStart = /\n#{2,3} Section /g;
  sectionStart.lastIndex = Math.floor(text.length / 2) - WINDOW_BYTES / 2;
  const start = sectionStart.exec(text);
  expect(start).not.toBeNull();
  sectionStart.lastIndex = start!.index + WINDOW_BYTES;
  const end = sectionStart.exec(text);
  expect(end).not.toBeNull();
  return text.slice(start!.index + 1, end!.index + 1);
}

function offersLineHeightSample(line: HTMLElement): boolean {
  const text = line.textContent ?? "";
  if (text.length === 0 || text.length > SAMPLE_MAX_LENGTH || /[^ -~]/.test(text)) return false;
  return [...line.childNodes].every((node) => node.nodeType === Node.TEXT_NODE);
}

function scanRenderedLines(target: EditorView, scan: SampleScan): void {
  for (const line of target.contentDOM.querySelectorAll<HTMLElement>(".cm-line")) {
    if (!NON_PROSE_HEIGHT_LINE_CLASSES.some((cls) => line.classList.contains(cls))) continue;
    scan.nonProseLines++;
    if (offersLineHeightSample(line)) scan.sampled.set(target.posAtDOM(line), line.textContent ?? "");
  }
}

function createMarkdownView(doc: string): EditorView {
  const state = EditorState.create({
    doc,
    extensions: [
      markdown({ base: markdownLanguage, extensions: MARKDOWN_EXTENSIONS }),
      syntaxHighlighting(jotraHighlight),
      markdownInlineExtension(NO_IMAGES, NEVER_RENDERS),
    ],
  });
  expect(ensureSyntaxTree(state, doc.length, PARSE_TIMEOUT_MS)?.length).toBe(doc.length);
  return new EditorView({ state, parent: document.body });
}

async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) await Promise.resolve();
}

let view: EditorView | null = null;

afterEach(() => {
  view?.destroy();
  view = null;
});

describe("CodeMirror's line-height sample in a Markdown note", () => {
  it("never comes from a line sized away from prose, across the gate's note", async () => {
    const doc = shiftedNoteMiddle();
    view = createMarkdownView(doc);
    await settle();

    const scan: SampleScan = { nonProseLines: 0, sampled: new Map() };
    for (let pos = 0; ; ) {
      scanRenderedLines(view, scan);
      if (view.viewport.to >= doc.length) break;
      pos = view.viewport.to + 1;
      view.dispatch({ effects: EditorView.scrollIntoView(pos, { y: "start" }) });
      await settle();
      expect(view.viewport.to).toBeGreaterThan(pos);
    }

    expect(scan.nonProseLines).toBeGreaterThan(0);
    expect([...new Set(scan.sampled.values())]).toEqual([]);
  });

  it.each(HAND_WRITTEN_BLOCKS)(
    "never comes from %s lines, with the caret outside the block or on each of its lines",
    async (_name, block) => {
      const doc = `Before\n\n${block}\n\nAfter\n`;
      view = createMarkdownView(doc);
      const blockStart = doc.indexOf(block);
      const firstLine = view.state.doc.lineAt(blockStart).number;
      const lastLine = view.state.doc.lineAt(blockStart + block.length).number;
      const carets = [0];
      for (let n = firstLine; n <= lastLine; n++) carets.push(view.state.doc.line(n).from);

      const scan: SampleScan = { nonProseLines: 0, sampled: new Map() };
      for (const caret of carets) {
        view.dispatch({ selection: { anchor: caret } });
        await settle();
        scanRenderedLines(view, scan);
      }

      expect(scan.nonProseLines).toBeGreaterThan(0);
      expect([...new Set(scan.sampled.values())]).toEqual([]);
    },
  );
});
