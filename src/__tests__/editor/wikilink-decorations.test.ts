import { describe, it, expect, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { findLinkTargets } from "../../editor/link-layer";
import {
  wikilinkDecorations,
  wikilinkDecorationLayer,
  WIKILINK_CLASS,
  WIKILINK_MISSING_CLASS,
  WIKILINK_RESOLVED_CLASS,
  type WikilinkDeps,
  type WikilinkStatus,
} from "../../editor/wikilink-decorations";

function stateFor(doc: string): EditorState {
  return EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] });
}

/** Every decoration class in the set, in document order. */
function classesOf(state: EditorState, statusOf: (target: string) => WikilinkStatus | null) {
  const ranges = findLinkTargets(state, 0, state.doc.length);
  const set = wikilinkDecorations(state, ranges, statusOf);
  const out: string[] = [];
  const cursor = set.iter();
  while (cursor.value) {
    out.push((cursor.value.spec as { class: string }).class);
    cursor.next();
  }
  return out;
}

describe("wikilinkDecorations", () => {
  it("gives a resolved and an unresolved link different classes", () => {
    const state = stateFor("[[Here]] and [[Nowhere]]");
    const classes = classesOf(state, (target) =>
      target === "Here" ? "resolved" : "missing",
    );
    expect(classes).toEqual([
      `${WIKILINK_CLASS} ${WIKILINK_RESOLVED_CLASS}`,
      `${WIKILINK_CLASS} ${WIKILINK_MISSING_CLASS}`,
    ]);
  });

  it("paints an ambiguous target as unresolved, not as a destination", () => {
    const state = stateFor("[[Both]]");
    expect(classesOf(state, () => "ambiguous")).toEqual([
      `${WIKILINK_CLASS} ${WIKILINK_MISSING_CLASS}`,
    ]);
  });

  // Resolution is a round trip, so a link with no answer yet must not be
  // painted as missing and then flip: it carries the base class alone.
  it("paints a target with no answer yet as neither", () => {
    const state = stateFor("[[Pending]]");
    expect(classesOf(state, () => null)).toEqual([WIKILINK_CLASS]);
  });

  it("paints nothing for a markdown link", () => {
    const state = stateFor("[label](./other.md) and https://example.com/x");
    expect(classesOf(state, () => "resolved")).toEqual([]);
  });
});

describe("wikilinkDecorationLayer", () => {
  function mount(doc: string, deps: WikilinkDeps) {
    const state = EditorState.create({
      doc,
      extensions: [markdown({ base: markdownLanguage }), wikilinkDecorationLayer(deps)],
    });
    return new EditorView({ state, parent: document.body });
  }

  it("asks once per target and repaints when the answer lands", async () => {
    const known = new Map<string, WikilinkStatus>();
    const resolve = vi.fn(async (_from: string, target: string) => {
      known.set(target, target === "Here" ? "resolved" : "missing");
    });
    const view = mount("[[Here]] and [[Nowhere]]", {
      fromPath: () => "/notes/From.md",
      known: (_from, target) => known.get(target) ?? null,
      resolve,
      generation: () => 0,
    });

    expect(resolve).toHaveBeenCalledTimes(2);
    expect(resolve).toHaveBeenCalledWith("/notes/From.md", "Here");
    expect(resolve).toHaveBeenCalledWith("/notes/From.md", "Nowhere");

    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    const painted = view.dom.querySelectorAll(`.${WIKILINK_CLASS}`);
    expect(painted.length).toBe(2);
    expect(painted[0].classList.contains(WIKILINK_RESOLVED_CLASS)).toBe(true);
    expect(painted[1].classList.contains(WIKILINK_MISSING_CLASS)).toBe(true);
    // The repaint must not start another round of reads.
    expect(resolve).toHaveBeenCalledTimes(2);
    view.destroy();
  });

  it("asks nothing for a note with no file", () => {
    const resolve = vi.fn(async () => undefined);
    const view = mount("[[Here]]", {
      fromPath: () => null,
      known: () => null,
      resolve,
      generation: () => 0,
    });
    expect(resolve).not.toHaveBeenCalled();
    view.destroy();
  });

  // Creating the note a link named empties the cache. Without the generation
  // the layer would remember having asked, keep reading `null`, and leave the
  // new note's link painted as neither for the life of the view.
  it("asks again once the cache says it was emptied", async () => {
    const known = new Map<string, WikilinkStatus>([["New", "missing"]]);
    let generation = 0;
    const resolve = vi.fn(async (_from: string, target: string) => {
      known.set(target, "resolved");
    });
    const view = mount("[[New]]", {
      fromPath: () => "/notes/From.md",
      known: (_from, target) => known.get(target) ?? null,
      resolve,
      generation: () => generation,
    });
    expect(resolve).not.toHaveBeenCalled();

    known.delete("New");
    generation = 1;
    view.dispatch({ changes: { from: view.state.doc.length, insert: " " } });

    expect(resolve).toHaveBeenCalledTimes(1);
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    expect(
      view.dom.querySelector(`.${WIKILINK_CLASS}`)?.classList.contains(
        WIKILINK_RESOLVED_CLASS,
      ),
    ).toBe(true);
    view.destroy();
  });
});
