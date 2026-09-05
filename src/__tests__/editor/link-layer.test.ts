import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { defaultKeymap } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import {
  findLinkTargets,
  linkClickTarget,
  linkLayer,
  mergeLinkRanges,
  modifierIsHeld,
  trimUrlTail,
  wikilinkAtCursor,
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

describe("findLinkTargets over wikilinks", () => {
  // The range is the target itself, so a click hands on exactly what the
  // resolver parses and the decoration paints the text rather than the
  // brackets around it.
  function wikilinks(doc: string): string[] {
    const state = markdownState(doc);
    return targets(state)
      .filter((r) => r.kind === "wikilink")
      .map((r) => textOf(state, r));
  }

  it("finds every wikilink form", () => {
    expect(wikilinks("[[Note]]")).toEqual(["Note"]);
    expect(wikilinks("[[Note|alias]]")).toEqual(["Note|alias"]);
    expect(wikilinks("[[Note#Heading]]")).toEqual(["Note#Heading"]);
    expect(wikilinks("[[folder/Note]]")).toEqual(["folder/Note"]);
  });

  it("finds a markdown .md link beside a wikilink and keeps them apart", () => {
    const state = markdownState("[[Note]] and [label](./other.md)");
    const found = targets(state);
    expect(found.map((r) => [textOf(state, r), r.kind])).toEqual([
      ["Note", "wikilink"],
      ["./other.md", "path"],
    ]);
  });

  it("finds both wikilinks on one line", () => {
    expect(wikilinks("[[A]] then [[B]]")).toEqual(["A", "B"]);
  });

  it("is not an embed, an empty target, or an unclosed run", () => {
    expect(wikilinks("![[a.png]]")).toEqual([]);
    expect(wikilinks("[[]]")).toEqual([]);
    expect(wikilinks("[[   ]]")).toEqual([]);
    expect(wikilinks("[[unclosed")).toEqual([]);
  });

  // The target ends at the first `]]`, which is what writ-core's scanner does,
  // so a bracket written inside one is part of the name in both.
  it("ends a target at the first close, as the index does", () => {
    expect(wikilinks("[[a]b]]")).toEqual(["a]b"]);
    expect(wikilinks("[[a[b]]")).toEqual(["a[b"]);
    expect(wikilinks("[[Note|al]]ias]]")).toEqual(["Note|al"]);
  });

  // Code is an example, not a destination. The index and the preview both
  // leave it literal.
  it("finds nothing inside fenced, indented or inline code", () => {
    expect(wikilinks("```\n[[Note]]\n```")).toEqual([]);
    expect(wikilinks("~~~md\n[[Note]]\n~~~")).toEqual([]);
    expect(wikilinks("    [[Note]]")).toEqual([]);
    expect(wikilinks("write `[[Note]]` to link")).toEqual([]);
    expect(wikilinks("`code` then [[Note]]")).toEqual(["Note"]);
  });

  it("wins the overlap against a bare address written inside it", () => {
    const state = markdownState("[[https://example.com/x]]");
    const found = targets(state);
    expect(found).toHaveLength(1);
    expect(found[0].kind).toBe("wikilink");
    expect(textOf(state, found[0])).toBe("https://example.com/x");
  });
});

// ─── Wiring, on a mounted view ─────────────────────────────────────────────

describe("linkLayer", () => {
  let view: EditorView;
  let deps: {
    openUrl: ReturnType<typeof vi.fn>;
    openWorkspaceFile: ReturnType<typeof vi.fn>;
    openNoteLink: ReturnType<typeof vi.fn>;
  };

  function mount(doc: string, extensions: unknown[] = []) {
    deps = {
      openUrl: vi.fn(),
      openWorkspaceFile: vi.fn(),
      openNoteLink: vi.fn().mockReturnValue(true),
    };
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

  it("routes a wikilink to the note dependency, target only", () => {
    mount("see [[folder/Note|alias]] now", [markdown({ base: markdownLanguage })]);
    clickAt(8, true);
    expect(deps.openNoteLink).toHaveBeenCalledWith("folder/Note|alias");
    expect(deps.openWorkspaceFile).not.toHaveBeenCalled();
    expect(deps.openUrl).not.toHaveBeenCalled();
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

  function pressEnter(): KeyboardEvent {
    const event = new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      key: "Enter",
      code: "Enter",
    });
    view.contentDOM.dispatchEvent(event);
    return event;
  }

  // The newline binding claims every Enter and is mounted with the rest of the
  // editor's keys, so the test carries it too: without the precedence the link
  // key never runs.
  const withNewline = [markdown({ base: markdownLanguage }), keymap.of(defaultKeymap)];

  it("opens the note the caret is inside on Enter", () => {
    mount("see [[Grocery list]] now", withNewline);
    view.dispatch({ selection: { anchor: 10 } });
    expect(pressEnter().defaultPrevented).toBe(true);
    expect(deps.openNoteLink).toHaveBeenCalledWith("Grocery list");
    expect(view.state.doc.lines).toBe(1);
  });

  it("leaves Enter alone everywhere else", () => {
    mount("see [[Grocery list]] now", withNewline);
    view.dispatch({ selection: { anchor: 22 } });
    pressEnter();
    expect(deps.openNoteLink).not.toHaveBeenCalled();
    expect(view.state.doc.lines).toBe(2);
  });

  // A note documenting Writ's own link syntax writes `[[…]]` inside a fence.
  // Enter there adds a line; taking the keystroke would lose it and open the
  // picker over what is being written.
  it.each([
    ["fenced", "```\n[[Note]]\n```\n", 6],
    ["indented", "    [[Note]]\n", 8],
    ["inline", "write `[[Note]]` to link\n", 11],
  ])("breaks the line inside %s code", (_kind, doc, caret) => {
    mount(doc, withNewline);
    const before = view.state.doc.lines;
    view.dispatch({ selection: { anchor: caret } });
    pressEnter();
    expect(deps.openNoteLink).not.toHaveBeenCalled();
    expect(view.state.doc.lines).toBe(before + 1);
  });

  // A note with no file cannot resolve a target, and a swallowed Enter that
  // neither opens nor breaks the line is worse than either.
  it("still breaks the line when the note cannot be resolved from", () => {
    mount("see [[Grocery list]] now", withNewline);
    deps.openNoteLink.mockReturnValue(false);
    view.dispatch({ selection: { anchor: 10 } });
    pressEnter();
    expect(deps.openNoteLink).toHaveBeenCalledWith("Grocery list");
    expect(view.state.doc.lines).toBe(2);
  });
});

// ─── Following a link from the keyboard ────────────────────────────────────

describe("wikilinkAtCursor", () => {
  function stateWith(doc: string, head: number): EditorState {
    return EditorState.create({
      doc,
      selection: { anchor: head },
      extensions: [markdown({ base: markdownLanguage })],
    });
  }

  it("names the target the caret sits in", () => {
    expect(wikilinkAtCursor(stateWith("see [[Grocery list]] now", 10))).toBe(
      "Grocery list",
    );
  });

  // At either edge the pair has only just been typed or is about to be closed,
  // so Enter there stays a line break.
  it("is null at the edges, outside, and on a selection", () => {
    expect(wikilinkAtCursor(stateWith("[[Note]]", 2))).toBeNull();
    expect(wikilinkAtCursor(stateWith("[[Note]]", 6))).toBeNull();
    expect(wikilinkAtCursor(stateWith("[[Note]] tail", 11))).toBeNull();
    expect(wikilinkAtCursor(stateWith("plain [text](./x.md)", 8))).toBeNull();
    expect(
      wikilinkAtCursor(
        EditorState.create({
          doc: "[[Note]]",
          selection: { anchor: 3, head: 5 },
          extensions: [markdown({ base: markdownLanguage })],
        }),
      ),
    ).toBeNull();
  });
});
