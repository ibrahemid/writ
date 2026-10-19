import { afterEach, describe, expect, it, vi } from "vitest";
import { Compartment, EditorState, StateEffect, StateField, type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { SearchQuery, search, setSearchQuery } from "@codemirror/search";
import { SYNC_SCAN_MAX_CHARS, findMatches, findMatchesPlugin } from "../../editor/search/find-matches";
import { MatchIndex } from "../../editor/search/match-index";
import { createQueryScanner } from "../../editor/search/scanner";
import { engineGivesUpOn } from "./fixtures/regexp-gives-up";

/** Slices the plugin asks for, run only when the test says so. */
function manualSlices() {
  const queue: { run: () => void; cancelled: boolean }[] = [];
  const schedule = (slice: () => void) => {
    const entry = { run: slice, cancelled: false };
    queue.push(entry);
    return () => {
      entry.cancelled = true;
    };
  };
  const runNext = (): boolean => {
    const entry = queue.shift();
    if (!entry) return false;
    if (!entry.cancelled) entry.run();
    return true;
  };
  return { queue, schedule, runNext };
}

/** Work the plugin puts off until a frame is on screen, run when the test draws one. */
function manualFrames() {
  const queue: { run: () => void; cancelled: boolean }[] = [];
  const afterFrame = (work: () => void) => {
    const entry = { run: work, cancelled: false };
    queue.push(entry);
    return () => {
      entry.cancelled = true;
    };
  };
  const draw = (): void => {
    for (const entry of queue.splice(0)) if (!entry.cancelled) entry.run();
  };
  return { queue, afterFrame, draw };
}

const views: EditorView[] = [];

function makeView(doc: string, extensions: Extension[]): EditorView {
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  const view = new EditorView({ state: EditorState.create({ doc, extensions: [search({ top: true }), ...extensions] }), parent });
  views.push(view);
  return view;
}

function setQuery(view: EditorView, spec: ConstructorParameters<typeof SearchQuery>[0]): void {
  view.dispatch({ effects: setSearchQuery.of(new SearchQuery(spec)) });
}

function plugin(view: EditorView) {
  const found = view.plugin(findMatchesPlugin);
  if (!found) throw new Error("the view has no find plugin");
  return found;
}

/** A document over the size counted in one go: "needle" on every tenth line. */
function longDocument(): { text: string; needles: number } {
  const lines: string[] = [];
  let needles = 0;
  for (let i = 0; lines.join("\n").length <= SYNC_SCAN_MAX_CHARS * 1.5 && i < 1e6; i += 1000) {
    for (let j = i; j < i + 1000; j++) {
      if (j % 10 === 0) {
        lines.push(`line ${j} has a needle in it`);
        needles++;
      } else {
        lines.push(`line ${j} has nothing in it`);
      }
    }
  }
  return { text: lines.join("\n"), needles };
}

interface Span {
  from: number;
  to: number;
}

/**
 * The visible ranges CodeMirror leaves around the hidden part of a long line
 * shown without wrapping. jsdom lays nothing out, so the test sets them.
 */
function showOnly(view: EditorView, ranges: readonly Span[]): void {
  Object.defineProperty(view, "visibleRanges", { get: () => ranges, configurable: true });
}

/** Forty short lines, a line of 7,999 characters, forty short lines; visible: the top and the start of the second block. */
function splitByLongLine(): { doc: string; ranges: Span[] } {
  const short = Array.from({ length: 40 }, (_, i) => `line ${i} has some words in it here and there`).join("\n");
  const long = Array.from({ length: 1600 }, (_, i) => `"w${i % 10}"`).join(" ");
  const longFrom = short.length + 1;
  const longTo = longFrom + long.length;
  return {
    doc: `${short}\n${long}\n${short}`,
    ranges: [
      { from: 0, to: longFrom + 4000 },
      { from: longTo, to: longTo + 600 },
    ],
  };
}

/** Visible ranges that move with the edits, the way CodeMirror maps its own. */
function followVisible(view: EditorView, initial: (length: number) => Span[]): void {
  const visible = StateField.define<Span[]>({
    create: (state) => initial(state.doc.length),
    update: (ranges, tr) =>
      tr.docChanged
        ? ranges.map((range) => ({ from: tr.changes.mapPos(range.from, -1), to: tr.changes.mapPos(range.to, 1) }))
        : ranges,
  });
  view.dispatch({ effects: StateEffect.appendConfig.of(visible) });
  Object.defineProperty(view, "visibleRanges", { get: () => view.state.field(visible), configurable: true });
}

/** Every mark the find highlighter paints, with its class. */
function markedSpans(view: EditorView): { from: number; to: number; selected: boolean }[] {
  const spans: { from: number; to: number; selected: boolean }[] = [];
  for (const source of view.state.facet(EditorView.decorations)) {
    const set = typeof source === "function" ? source(view) : source;
    for (const cursor = set.iter(); cursor.value; cursor.next()) {
      const classes = String(cursor.value.spec.class ?? "").split(" ");
      if (classes.includes("cm-searchMatch")) {
        spans.push({ from: cursor.from, to: cursor.to, selected: classes.includes("cm-searchMatch-selected") });
      }
    }
  }
  return spans;
}

function paintedMatches(view: EditorView): Span[] {
  const painted: Span[] = [];
  for (const source of view.state.facet(EditorView.decorations)) {
    const set = typeof source === "function" ? source(view) : source;
    for (const cursor = set.iter(); cursor.value; cursor.next()) {
      if (String(cursor.value.spec.class ?? "").split(" ").includes("cm-searchMatch")) {
        painted.push({ from: cursor.from, to: cursor.to });
      }
    }
  }
  return painted;
}

function coverage(length: number, spans: readonly Span[]): Uint8Array {
  const covered = new Uint8Array(length);
  for (const span of spans) covered.fill(1, span.from, span.to);
  return covered;
}

/**
 * Holds the painted marks to the expression's matches over the whole text:
 * each mark is a match starting within the highlighter's margin of a visible
 * range, marks do not overlap, and every visible character is painted exactly
 * when one of those matches covers it.
 */
function expectPaintedAsMatched(view: EditorView, query: { search: string; regexp: boolean }, ranges: readonly Span[]): void {
  const doc = view.state.doc.toString();
  const source = query.regexp ? query.search : query.search.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const margin = query.regexp ? 250 : query.search.length;
  const whole = new RegExp(`^(?:${source})$`, "iu");
  const painted = paintedMatches(view);
  painted.forEach((span, i) => {
    const where = `${query.search} at ${span.from}-${span.to}`;
    expect(whole.test(doc.slice(span.from, span.to)), `${where} is not a match`).toBe(true);
    if (i > 0) expect(span.from, `${where} overlaps the mark before it`).toBeGreaterThanOrEqual(painted[i - 1].to);
    expect(
      ranges.some((range) => span.from >= range.from - margin && span.from < range.to + margin),
      `${where} is outside every visible range and its margin`,
    ).toBe(true);
  });
  const matched = [...doc.matchAll(new RegExp(source, "gimu"))].map((m) => ({ from: m.index, to: m.index + m[0].length }));
  const expected = coverage(doc.length, matched);
  const actual = coverage(doc.length, painted);
  const wrong: number[] = [];
  for (const range of ranges) {
    for (let pos = range.from; pos < range.to; pos++) if (expected[pos] !== actual[pos]) wrong.push(pos);
  }
  expect(wrong.slice(0, 10), `${query.search}: visible positions painted unlike the matches`).toEqual([]);
}

afterEach(() => {
  vi.restoreAllMocks();
  while (views.length) views.pop()?.destroy();
});

describe("findMatches", () => {
  it("counts a document up to 1 MiB in the update that sets the query", () => {
    const slices = manualSlices();
    const view = makeView("needle hay needle\nhay needle", [findMatches({ schedule: slices.schedule })]);
    setQuery(view, { search: "needle" });
    expect(plugin(view).matchState()).toMatchObject({ total: 3, pending: false });
    expect(slices.queue).toHaveLength(0);
  });

  it("reads a longer document a slice at a time, the count growing until it is final", () => {
    const { text, needles } = longDocument();
    const slices = manualSlices();
    const onChange = vi.fn();
    const view = makeView(text, [findMatches({ schedule: slices.schedule, onChange })]);
    setQuery(view, { search: "needle" });
    expect(plugin(view).matchState()).toMatchObject({ total: 0, pending: true });

    // Every clock read is 10 ms later, so each slice runs out of time after one block.
    let clock = 0;
    vi.spyOn(performance, "now").mockImplementation(() => (clock += 10));
    const totals: number[] = [];
    let runs = 0;
    while (slices.runNext()) {
      runs++;
      totals.push(plugin(view).matchState().total);
    }
    expect(runs).toBeGreaterThan(5);
    expect(onChange).toHaveBeenCalledTimes(runs);
    expect(totals).toEqual([...totals].sort((a, b) => a - b));
    expect(plugin(view).matchState()).toMatchObject({ total: needles, pending: false, capped: false });
  });

  it("tells the listener once the update that set the query has finished", async () => {
    const onChange = vi.fn();
    const view = makeView("needle needle", [findMatches({ onChange })]);
    setQuery(view, { search: "needle" });
    expect(onChange).not.toHaveBeenCalled();
    await Promise.resolve();
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(plugin(view).matchState().total).toBe(2);
  });

  it("tells the listener about edits once the frame drawing them is on screen, once per frame", async () => {
    const onChange = vi.fn();
    const frames = manualFrames();
    const view = makeView("needle", [findMatches({ onChange, afterFrame: frames.afterFrame })]);
    setQuery(view, { search: "needle" });
    await Promise.resolve();
    onChange.mockClear();

    view.dispatch({ changes: { from: 0, insert: "needle " } });
    view.dispatch({ changes: { from: 0, insert: "needle " } });
    expect(plugin(view).matchState()).toMatchObject({ total: 3, pending: false });
    await Promise.resolve();
    expect(onChange).not.toHaveBeenCalled();

    frames.draw();
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it("reads the edited part of a longer document once the frame drawing the edit is on screen", () => {
    const { text, needles } = longDocument();
    const slices = manualSlices();
    const frames = manualFrames();
    const onChange = vi.fn();
    const view = makeView(text, [findMatches({ schedule: slices.schedule, afterFrame: frames.afterFrame, onChange })]);
    setQuery(view, { search: "needle" });
    while (slices.runNext());
    expect(plugin(view).matchState()).toMatchObject({ total: needles, pending: false });
    onChange.mockClear();

    const middle = view.state.doc.line(Math.floor(view.state.doc.lines / 2)).to;
    view.dispatch({ changes: { from: middle, insert: " needle" } });
    expect(slices.queue).toHaveLength(0);
    expect(frames.queue).toHaveLength(1);
    expect(onChange).not.toHaveBeenCalled();

    frames.draw();
    while (slices.runNext());
    expect(plugin(view).matchState()).toMatchObject({ total: needles + 1, pending: false });
    expect(onChange).toHaveBeenCalled();
  });

  it("holds the count and positions to a fresh reading after edits drawn across frames", () => {
    const { text } = longDocument();
    const slices = manualSlices();
    const frames = manualFrames();
    const view = makeView(text, [findMatches({ schedule: slices.schedule, afterFrame: frames.afterFrame })]);
    setQuery(view, { search: "Needle" });
    while (slices.runNext());

    let seed = 7;
    const random = (below: number) => {
      seed = (seed * 1_103_515_245 + 12_345) % 2_147_483_648;
      return seed % below;
    };
    const pieces = ["needle", "nee", "dle", " ", "\n", "NEEDLE ", "x"];
    for (let edit = 0; edit < 60; edit++) {
      const length = view.state.doc.length;
      const from = random(length);
      const to = random(3) === 0 ? Math.min(length, from + random(40)) : from;
      view.dispatch({ changes: { from, to, insert: pieces[random(pieces.length)] } });
      if (random(3) === 0) frames.draw();
      if (random(4) === 0) slices.runNext();
    }
    frames.draw();
    while (slices.runNext());

    const fresh = new MatchIndex(createQueryScanner(new SearchQuery({ search: "Needle" }))!);
    fresh.advance(view.state, Infinity);
    expect(plugin(view).matchState()).toMatchObject({ total: fresh.count, pending: false });
    expect(plugin(view).ticks(Infinity)).toEqual(fresh.ticks(Infinity, view.state.doc.length));
  });

  it("counts a match typed into the document and drops one deleted from it", () => {
    const view = makeView("needle hay\nhay", [findMatches()]);
    setQuery(view, { search: "needle" });
    view.dispatch({ changes: { from: view.state.doc.length, insert: " needle" } });
    expect(plugin(view).matchState().total).toBe(2);
    view.dispatch({ changes: { from: 0, to: 6 } });
    expect(plugin(view).matchState().total).toBe(1);
  });

  it("counts a whole-word query again when the language changes what a word is", () => {
    const words = new Compartment();
    const view = makeView("foo-bar foo bar", [findMatches(), words.of([])]);
    setQuery(view, { search: "foo", wholeWord: true });
    expect(plugin(view).matchState().total).toBe(2);
    view.dispatch({
      effects: words.reconfigure(EditorState.languageData.of(() => [{ wordChars: "-" }])),
    });
    expect(plugin(view).matchState().total).toBe(1);
  });

  it("stops reading when the editor is destroyed", () => {
    const slices = manualSlices();
    const onChange = vi.fn();
    const view = makeView(longDocument().text, [findMatches({ schedule: slices.schedule, onChange })]);
    setQuery(view, { search: "needle" });
    expect(slices.queue).toHaveLength(1);
    view.destroy();
    expect(slices.queue[0].cancelled).toBe(true);
  });

  it("has found nothing only once the whole document has been read", () => {
    const slices = manualSlices();
    const view = makeView(longDocument().text, [findMatches({ schedule: slices.schedule })]);
    setQuery(view, { search: "haystack" });
    expect(plugin(view).foundNothing()).toBe(false);
    while (slices.runNext());
    expect(plugin(view).foundNothing()).toBe(true);
  });

  it("reports a pattern that does not compile without reading the document", () => {
    const slices = manualSlices();
    const view = makeView(longDocument().text, [findMatches({ schedule: slices.schedule })]);
    setQuery(view, { search: "(", regexp: true });
    expect(plugin(view).matchState()).toMatchObject({ total: 0, invalid: true, pending: false });
    expect(slices.queue).toHaveLength(0);
  });

  it("reports a pattern the regular expression engine gives up on, and counts again once it can", () => {
    engineGivesUpOn(["(\\D)+", "(.)+"], (text) => text.includes("~"));
    const view = makeView("alpha ~ beta\ngamma", [findMatches()]);
    for (const search of ["(\\D)+", "(.)+"]) {
      setQuery(view, { search, regexp: true });
      expect(plugin(view).matchState(), search).toMatchObject({ total: 0, pending: false, failed: true });
      expect(plugin(view).foundNothing(), search).toBe(true);
      expect(plugin(view).ticks(10), search).toEqual([]);
      view.dispatch({ changes: { from: 0, insert: "x" } });
      expect(plugin(view).matchState(), search).toMatchObject({ total: 0, pending: false, failed: true });
    }
    const tilde = view.state.doc.toString().indexOf("~");
    view.dispatch({ changes: { from: tilde, to: tilde + 1 } });
    expect(plugin(view).matchState()).toMatchObject({ total: 2, pending: false, failed: false });
    setQuery(view, { search: "a" });
    expect(plugin(view).matchState()).toMatchObject({ total: 5, failed: false });
    expect(view.dom.querySelectorAll(".cm-searchMatch").length).toBeGreaterThan(0);
  });

  it("settles the slice the regular expression engine gives up in, and says so until a reading succeeds", () => {
    let givesUp = true;
    engineGivesUpOn(["(\\D)+"], () => givesUp);
    const slices = manualSlices();
    const frames = manualFrames();
    const onChange = vi.fn();
    const view = makeView(longDocument().text, [findMatches({ schedule: slices.schedule, afterFrame: frames.afterFrame, onChange })]);
    setQuery(view, { search: "(\\D)+", regexp: true });
    expect(plugin(view).matchState()).toMatchObject({ pending: true, failed: false });
    while (slices.runNext());
    expect(plugin(view).matchState()).toMatchObject({ total: 0, pending: false, failed: true });
    expect(onChange).toHaveBeenCalled();
    givesUp = false;
    view.dispatch({ changes: { from: 0, insert: "x" } });
    expect(plugin(view).matchState()).toMatchObject({ pending: true, failed: true });
    expect(plugin(view).foundNothing()).toBe(true);
    frames.draw();
    while (slices.runNext());
    expect(plugin(view).matchState()).toMatchObject({ pending: false, failed: false });
    expect(plugin(view).matchState().total).toBeGreaterThan(0);
  });

  it("drops the matches read before the engine gave up", () => {
    engineGivesUpOn(["(.)+"], (text) => text.includes("~"));
    const state = EditorState.create({ doc: "alpha\n~ beta\ngamma" });
    const index = new MatchIndex(createQueryScanner(new SearchQuery({ search: "(.)+", regexp: true }))!);
    index.advance(state, Infinity);
    expect({ settled: index.settled, failed: index.failed, count: index.count }).toEqual({ settled: true, failed: true, count: 0 });
    expect(index.ticks(10, state.doc.length)).toEqual([]);
  });

  it("gives up on a query carrying its own test where CodeMirror's cursor does", () => {
    engineGivesUpOn(["(\\D)+"]);
    const state = EditorState.create({ doc: "alpha beta" });
    const index = new MatchIndex(createQueryScanner(new SearchQuery({ search: "(\\D)+", regexp: true, test: () => true }))!);
    index.advance(state, Infinity);
    expect({ settled: index.settled, failed: index.failed, count: index.count }).toEqual({ settled: true, failed: true, count: 0 });
  });

  it("gives up where V8 does, on a match attempt too long for its backtracking stack", () => {
    const search = "(\\D)+";
    let doc = "";
    let threw = false;
    for (let size = 4 << 20; size <= 64 << 20 && !threw; size *= 2) {
      doc = "alpha beta gamma\n".repeat(Math.ceil(size / 17));
      try {
        new RegExp(search, "gmu").exec(doc);
      } catch (error) {
        threw = error instanceof RangeError;
      }
    }
    expect(threw, "a bare exec of (\\D)+ over a document without digits never threw up to 64 MiB").toBe(true);
    const state = EditorState.create({ doc });
    const query = new SearchQuery({ search, regexp: true });
    expect(() => query.getCursor(state).next()).toThrow(RangeError);
    const index = new MatchIndex(createQueryScanner(query)!);
    index.advance(state, Infinity);
    expect({ settled: index.settled, failed: index.failed, count: index.count }).toEqual({ settled: true, failed: true, count: 0 });
  });
});

describe("findMatches highlights", () => {
  it.each([
    ["\\s", true],
    ["\\s+", true],
    ["\\W", true],
    ["\\W+", true],
    ['"[^"]*"', true],
    ["\\w+", true],
    ["w1", false],
  ] as const)("paints %s in both visible ranges a long line splits, and nothing far from them", (search, regexp) => {
    const { doc, ranges } = splitByLongLine();
    const errors: unknown[] = [];
    const view = makeView(doc, [findMatches(), EditorView.exceptionSink.of((error) => errors.push(error))]);
    showOnly(view, ranges);
    setQuery(view, { search, regexp });
    expect(errors.map(String)).toEqual([]);
    expect(paintedMatches(view).length).toBeGreaterThan(0);
    expectPaintedAsMatched(view, { search, regexp }, ranges);
    setQuery(view, { search: "line", regexp: false });
    expect(errors.map(String)).toEqual([]);
    expect(paintedMatches(view).some((span) => span.from >= ranges[1].from)).toBe(true);
    expectPaintedAsMatched(view, { search: "line", regexp: false }, ranges);
  });

  it("searches visible ranges closer than twice a long literal's length as one", () => {
    const errors: unknown[] = [];
    const view = makeView(`${"b".repeat(400)}${"a".repeat(2000)}\n${"c".repeat(100)}`, [
      findMatches(),
      EditorView.exceptionSink.of((error) => errors.push(error)),
    ]);
    showOnly(view, [
      { from: 0, to: 1000 },
      { from: 1550, to: 2000 },
    ]);
    setQuery(view, { search: "a".repeat(600) });
    expect(errors.map(String)).toEqual([]);
    expect(paintedMatches(view)).toEqual([
      { from: 400, to: 1000 },
      { from: 1000, to: 1600 },
      { from: 1600, to: 2200 },
    ]);
  });

  it("searches only the lines an edit touched, for a literal on one line", () => {
    const lines = Array.from({ length: 200 }, (_, i) => `line ${i} holds a needle and some hay`);
    const view = makeView(lines.join("\n"), [findMatches()]);
    followVisible(view, (length) => [{ from: 0, to: length }]);
    setQuery(view, { search: "needle" });
    const cursor = vi.spyOn(SearchQuery.prototype, "getCursor");

    const at = view.state.doc.line(100).from + 5;
    view.dispatch({ changes: { from: at, insert: "x" }, selection: { anchor: at + 1 } });
    const line = view.state.doc.line(100);
    expect(cursor.mock.calls.length).toBeGreaterThan(0);
    for (const [, from, to] of cursor.mock.calls) {
      expect(from).toBeGreaterThanOrEqual(line.from);
      expect(to).toBeLessThanOrEqual(line.to);
    }
    expect(paintedMatches(view)).toHaveLength(200);
  });

  it.each([
    [{ search: "needle" }],
    [{ search: "Needle", caseSensitive: true }],
    [{ search: "nee", wholeWord: true }],
    [{ search: "aa" }],
    [{ search: "strasse" }],
    [{ search: "dle\\nline" }],
    [{ search: "ne+dle", regexp: true }],
  ])("paints after each edit what searching the whole visible text again paints (%o)", (spec) => {
    const long = Array.from({ length: 600 }, (_, i) => (i % 7 === 0 ? "needle" : `w${i}`)).join(" ");
    const short = Array.from({ length: 30 }, (_, i) => `row ${i} NEEDLE nee-dle aaaa Straße needle`).join("\n");
    const view = makeView(`${short}\n${long}\n${short}`, [findMatches()]);
    const longFrom = short.length + 1;
    followVisible(view, (length) => [
      { from: 0, to: longFrom + 900 },
      { from: longFrom + 3000, to: length },
    ]);
    setQuery(view, spec);

    let seed = 11;
    const random = (below: number) => {
      seed = (seed * 1_103_515_245 + 12_345) % 2_147_483_648;
      return seed % below;
    };
    const pieces = ["needle", "nee", "dle", "a", " ", "\n", "N", "ß", "x"];
    for (let edit = 0; edit < 120; edit++) {
      const length = view.state.doc.length;
      const typing = edit % 3 !== 0;
      const from = typing ? view.state.selection.main.head : random(length + 1);
      const to = !typing && random(3) === 0 ? Math.min(length, from + random(30)) : from;
      const insert = pieces[random(pieces.length)];
      view.dispatch({ changes: { from, to, insert }, selection: { anchor: from + insert.length } });
      if (edit % 40 === 39) view.dispatch({ selection: { anchor: 0, head: Math.min(6, view.state.doc.length) } });
      if (edit % 40 === 0) view.dispatch({ selection: { anchor: random(view.state.doc.length) } });

      const fresh = makeView(view.state.doc.toString(), [findMatches()]);
      fresh.dispatch({ selection: view.state.selection });
      showOnly(fresh, view.visibleRanges);
      setQuery(fresh, spec);
      expect(markedSpans(view), `edit ${edit}`).toEqual(markedSpans(fresh));
      fresh.destroy();
      views.splice(views.indexOf(fresh), 1);
    }
  });
});
