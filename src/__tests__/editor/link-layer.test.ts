import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import {
  findLinkTargets,
  linkClickTarget,
  linkLayer,
  mergeLinkRanges,
  modifierIsHeld,
  trimUrlTail,
  type LinkDeps,
  type LinkRange,
} from "../../editor/link-layer";
import { IS_MAC } from "../../lib/platform";

function plainState(doc: string): EditorState {
  return EditorState.create({ doc });
}

function markdownState(doc: string): EditorState {
  return EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] });
}

function textOf(state: EditorState, range: LinkRange): string {
  return state.doc.sliceString(range.from, range.to);
}

function targets(state: EditorState): LinkRange[] {
  return findLinkTargets(state, 0, state.doc.length);
}

// ─── Decoration targets ────────────────────────────────────────────────────

describe("findLinkTargets", () => {
  it("finds a bare url in a plain buffer", () => {
    const state = plainState("see https://example.com/docs for more");
    const found = targets(state);
    expect(found).toHaveLength(1);
    expect(textOf(state, found[0])).toBe("https://example.com/docs");
    expect(found[0].kind).toBe("url");
    expect(found[0].from).toBe(4);
  });

  it("finds a mailto address", () => {
    const state = plainState("write to mailto:hi@example.com today");
    const found = targets(state);
    expect(found.map((r) => textOf(state, r))).toEqual(["mailto:hi@example.com"]);
  });

  it("finds every url on a multi-line document", () => {
    const state = plainState("https://a.example/\nplain text\nhttp://b.example/x\n");
    expect(targets(state).map((r) => textOf(state, r))).toEqual([
      "https://a.example/",
      "http://b.example/x",
    ]);
  });

  it("stops before sentence punctuation and an unbalanced bracket", () => {
    const state = plainState("(see https://example.com/a), and https://example.com/b.");
    expect(targets(state).map((r) => textOf(state, r))).toEqual([
      "https://example.com/a",
      "https://example.com/b",
    ]);
  });

  it("keeps a balanced bracket inside the address", () => {
    const state = plainState("https://ex.example/wiki/Foo_(bar) end");
    expect(targets(state).map((r) => textOf(state, r))).toEqual([
      "https://ex.example/wiki/Foo_(bar)",
    ]);
  });

  it("ignores a scheme glued to a preceding word", () => {
    expect(targets(plainState("xhttps://example.com"))).toEqual([]);
  });

  it("finds a url straddling a viewport edge in one piece", () => {
    const doc = "lead in https://example.com/a/very/long/path trailing";
    const state = plainState(doc);
    // A window that cuts the address in half still yields the whole run.
    const found = findLinkTargets(state, 0, 12);
    expect(found.map((r) => textOf(state, r))).toEqual([
      "https://example.com/a/very/long/path",
    ]);
  });

  it("reads a markdown link destination from the syntax tree", () => {
    const state = markdownState("[the docs](https://example.com/docs) here");
    const found = targets(state);
    expect(found).toHaveLength(1);
    expect(textOf(state, found[0])).toBe("https://example.com/docs");
    expect(found[0].kind).toBe("url");
  });

  it("classifies a schemeless markdown destination as a path", () => {
    const state = markdownState("[notes](./sub/notes.md)");
    const found = targets(state);
    expect(found).toHaveLength(1);
    expect(textOf(state, found[0])).toBe("./sub/notes.md");
    expect(found[0].kind).toBe("path");
  });

  it("strips the delimiters of a pointy-bracket destination", () => {
    const state = markdownState("[notes](<./my notes.md>)");
    const found = targets(state);
    expect(found.map((r) => textOf(state, r))).toEqual(["./my notes.md"]);
    expect(found[0].kind).toBe("path");
  });

  it("classifies an unknown scheme as a url so the policy refuses it", () => {
    const state = markdownState("[run](javascript:alert(1))");
    const found = targets(state);
    expect(found[0].kind).toBe("url");
  });

  it("treats a windows drive letter as a path, not a scheme", () => {
    const state = markdownState("[notes](C:/work/notes.md)");
    expect(targets(state)[0].kind).toBe("path");
  });

  it("skips an image destination", () => {
    const state = markdownState("![shot](./shot.png)");
    expect(targets(state)).toEqual([]);
  });

  it("emits one range for a markdown link, not one per source", () => {
    const state = markdownState("[docs](https://example.com/docs)");
    const found = targets(state);
    expect(found).toHaveLength(1);
    expect(textOf(state, found[0])).toBe("https://example.com/docs");
  });

  it("finds nothing in a document without links", () => {
    expect(targets(plainState("no addresses here at all"))).toEqual([]);
  });
});

describe("trimUrlTail", () => {
  it("drops trailing prose punctuation", () => {
    expect(trimUrlTail("https://a.example/x.")).toBe("https://a.example/x");
    expect(trimUrlTail("https://a.example/x,")).toBe("https://a.example/x");
    expect(trimUrlTail("https://a.example/x?!")).toBe("https://a.example/x");
  });

  it("keeps a path that ends in a word character", () => {
    expect(trimUrlTail("https://a.example/x")).toBe("https://a.example/x");
  });
});

describe("mergeLinkRanges", () => {
  it("sorts by start and drops overlaps, first entry winning", () => {
    const merged = mergeLinkRanges([
      { from: 5, to: 12, kind: "url" },
      { from: 0, to: 4, kind: "path" },
      { from: 6, to: 20, kind: "url" },
    ]);
    expect(merged).toEqual([
      { from: 0, to: 4, kind: "path" },
      { from: 5, to: 12, kind: "url" },
    ]);
  });

  it("drops empty ranges, which Decoration.mark refuses", () => {
    expect(mergeLinkRanges([{ from: 3, to: 3, kind: "url" }])).toEqual([]);
  });
});

// ─── Click decision ────────────────────────────────────────────────────────

describe("linkClickTarget", () => {
  const ranges: LinkRange[] = [{ from: 4, to: 10, kind: "url" }];

  it("returns nothing for a plain click on a link", () => {
    expect(linkClickTarget(ranges, 6, false, 0)).toBeNull();
  });

  it("returns the range for a modifier click on a link", () => {
    expect(linkClickTarget(ranges, 6, true, 0)).toEqual(ranges[0]);
  });

  it("returns nothing for a modifier click off a link, leaving add-cursor alone", () => {
    expect(linkClickTarget(ranges, 2, true, 0)).toBeNull();
    expect(linkClickTarget(ranges, 40, true, 0)).toBeNull();
  });

  it("ignores a non-primary button", () => {
    expect(linkClickTarget(ranges, 6, true, 2)).toBeNull();
  });

  it("ignores a click whose position is outside the content", () => {
    expect(linkClickTarget(ranges, null, true, 0)).toBeNull();
  });

  it("treats the range as half-open so a click past the last character misses", () => {
    expect(linkClickTarget(ranges, 4, true, 0)).toEqual(ranges[0]);
    expect(linkClickTarget(ranges, 9, true, 0)).toEqual(ranges[0]);
    expect(linkClickTarget(ranges, 10, true, 0)).toBeNull();
  });
});

// ─── Wiring, on a mounted view ─────────────────────────────────────────────

describe("linkLayer", () => {
  let view: EditorView;
  let deps: { openUrl: ReturnType<typeof vi.fn>; openWorkspaceFile: ReturnType<typeof vi.fn> };

  function mount(doc: string, extensions: unknown[] = []) {
    deps = { openUrl: vi.fn(), openWorkspaceFile: vi.fn() };
    const state = EditorState.create({
      doc,
      extensions: [
        EditorState.allowMultipleSelections.of(true),
        ...(extensions as never[]),
        linkLayer(deps as unknown as LinkDeps),
      ],
    });
    view = new EditorView({ state, parent: document.body });
    return view;
  }

  function clickAt(pos: number | null, modifier: boolean): MouseEvent {
    // jsdom has no layout, so the pointer-to-position mapping is supplied
    // directly; everything after it is the code under test.
    view.posAtCoords = ((): number | null => pos) as EditorView["posAtCoords"];
    const event = new MouseEvent("mousedown", {
      bubbles: true,
      cancelable: true,
      button: 0,
      metaKey: IS_MAC ? modifier : false,
      ctrlKey: IS_MAC ? false : modifier,
    });
    view.contentDOM.dispatchEvent(event);
    return event;
  }

  function pressModifier(type: "keydown" | "keyup", down: boolean) {
    view.contentDOM.dispatchEvent(
      new KeyboardEvent(type, {
        bubbles: true,
        key: IS_MAC ? "Meta" : "Control",
        metaKey: IS_MAC ? down : false,
        ctrlKey: IS_MAC ? false : down,
      }),
    );
  }

  beforeEach(() => {
    document.body.innerHTML = "";
  });

  afterEach(() => {
    view?.destroy();
  });

  it("decorates the url in the mounted document", () => {
    mount("go to https://example.com/x now");
    expect(view.dom.querySelector(".writ-link")?.textContent).toBe("https://example.com/x");
  });

  it("does not open on a plain click", () => {
    mount("go to https://example.com/x now");
    clickAt(8, false);
    expect(deps.openUrl).not.toHaveBeenCalled();
    expect(deps.openWorkspaceFile).not.toHaveBeenCalled();
  });

  it("opens once on a modifier click on the link", () => {
    mount("go to https://example.com/x now");
    clickAt(8, true);
    expect(deps.openUrl).toHaveBeenCalledTimes(1);
    expect(deps.openUrl).toHaveBeenCalledWith("https://example.com/x");
  });

  it("consumes the modifier click on a link", () => {
    mount("go to https://example.com/x now");
    // Consuming the event is what keeps CodeMirror's own modifier-click
    // add-cursor from also running. jsdom has no layout, so what CodeMirror
    // would have done with the click is not observable here; the precedence
    // rule itself is pinned on `linkClickTarget` above.
    expect(clickAt(8, true).defaultPrevented).toBe(true);
    expect(deps.openUrl).toHaveBeenCalledTimes(1);
  });

  it("leaves the modifier click alone off a link", () => {
    mount("go to https://example.com/x now");
    clickAt(2, true);
    expect(deps.openUrl).not.toHaveBeenCalled();
    expect(deps.openWorkspaceFile).not.toHaveBeenCalled();
  });

  it("opens on a modifier click that arrives while the modifier field is stale", () => {
    mount("go to https://example.com/x now");
    // The modifier went down before the editor had focus, so no keydown was
    // ever seen and the styling field is still false. The click still opens,
    // because the decision reads the event.
    expect(modifierIsHeld(view.state)).toBe(false);
    clickAt(8, true);
    expect(deps.openUrl).toHaveBeenCalledTimes(1);
  });

  it("routes a workspace-relative destination to the file dependency", () => {
    mount("[notes](./sub/notes.md)", [markdown({ base: markdownLanguage })]);
    clickAt(10, true);
    expect(deps.openWorkspaceFile).toHaveBeenCalledWith("./sub/notes.md");
    expect(deps.openUrl).not.toHaveBeenCalled();
  });

  it("routes a rejected scheme to the url dependency, which the policy refuses", () => {
    mount("[run](javascript:alert(1))", [markdown({ base: markdownLanguage })]);
    clickAt(7, true);
    expect(deps.openUrl).toHaveBeenCalledWith("javascript:alert(1)");
    expect(deps.openWorkspaceFile).not.toHaveBeenCalled();
  });

  it("tracks the modifier for styling and clears it on blur", () => {
    mount("go to https://example.com/x now");
    expect(modifierIsHeld(view.state)).toBe(false);

    pressModifier("keydown", true);
    expect(modifierIsHeld(view.state)).toBe(true);
    expect(view.dom.classList.contains("writ-link-active")).toBe(true);

    pressModifier("keyup", false);
    expect(modifierIsHeld(view.state)).toBe(false);

    pressModifier("keydown", true);
    expect(modifierIsHeld(view.state)).toBe(true);
    view.contentDOM.dispatchEvent(new FocusEvent("blur", { bubbles: false }));
    expect(modifierIsHeld(view.state)).toBe(false);
    expect(view.dom.classList.contains("writ-link-active")).toBe(false);
  });
});
