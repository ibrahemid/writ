import { describe, it, expect, vi } from "vitest";
import {
  buildEditorMenuItems,
  spellingAt,
  truncate,
  MAX_SUGGESTIONS,
  type EditorMenuActions,
  type EditorMenuContext,
} from "../../editor/context-menu";
import type { SpellingEntry } from "../../editor/spelling";

function actions(): EditorMenuActions {
  return {
    cut: vi.fn(),
    copy: vi.fn(),
    paste: vi.fn(),
    selectAll: vi.fn(),
    applySpelling: vi.fn(),
    addToDictionary: vi.fn(),
    openLink: vi.fn(),
    copyLink: vi.fn(),
    runRewrite: vi.fn(),
    fillPlaceholders: vi.fn(),
    searchWorkspace: vi.fn(),
    rewriteActions: [
      { id: "proofread", menuLabel: "Proofread" },
      { id: "rephrase", menuLabel: "Rephrase" },
      { id: "polish", menuLabel: "Polish" },
      { id: "improve_prompt", menuLabel: "Improve prompt" },
      { id: "custom", menuLabel: "Custom…" },
    ],
  };
}

function context(overrides: Partial<EditorMenuContext> = {}): EditorMenuContext {
  return {
    hasSelection: false,
    selectionText: "",
    spelling: null,
    link: null,
    aiEnabled: false,
    hasPlaceholders: false,
    editable: true,
    ...overrides,
  };
}

function entry(overrides: Partial<SpellingEntry> = {}): SpellingEntry {
  return {
    from: 10,
    to: 15,
    word: "teh",
    message: "Possible spelling mistake",
    kind: "spelling",
    suggestions: ["the", "tech", "ten"],
    confident: true,
    ...overrides,
  };
}

const labels = (items: { label: string }[]) => items.map((i) => i.label);

describe("editor menu adapts to the selection", () => {
  it("offers cut, copy and paste when text is selected", () => {
    const items = buildEditorMenuItems(context({ hasSelection: true, selectionText: "hi" }), actions());
    expect(labels(items)).toEqual(
      expect.arrayContaining(["Cut", "Copy", "Paste"]),
    );
    expect(labels(items)).not.toContain("Select all");
  });

  it("offers paste and select all when nothing is selected", () => {
    const items = buildEditorMenuItems(context(), actions());
    expect(labels(items)).toEqual(["Paste", "Select all"]);
  });

  it("disables the editing verbs in a read-only buffer but keeps copy", () => {
    const items = buildEditorMenuItems(
      context({ hasSelection: true, selectionText: "hi", editable: false }),
      actions(),
    );
    const byLabel = Object.fromEntries(items.map((i) => [i.label, i]));
    expect(byLabel["Cut"].disabled).toBe(true);
    expect(byLabel["Paste"].disabled).toBe(true);
    expect(byLabel["Copy"].disabled).toBeFalsy();
  });
});

describe("rewrite group", () => {
  it("lists every rewrite action when text is selected and the feature is on", () => {
    const items = buildEditorMenuItems(
      context({ hasSelection: true, selectionText: "hi", aiEnabled: true }),
      actions(),
    );
    expect(labels(items)).toEqual(
      expect.arrayContaining(["Proofread", "Rephrase", "Polish", "Improve prompt", "Custom…"]),
    );
  });

  it("is absent when the feature is off", () => {
    const items = buildEditorMenuItems(
      context({ hasSelection: true, selectionText: "hi", aiEnabled: false }),
      actions(),
    );
    expect(labels(items)).not.toContain("Proofread");
  });

  it("is absent with no selection, where the palette owns the whole-document path", () => {
    const items = buildEditorMenuItems(context({ aiEnabled: true }), actions());
    expect(labels(items)).not.toContain("Proofread");
  });

  it("runs the action it was given", () => {
    const acts = actions();
    const items = buildEditorMenuItems(
      context({ hasSelection: true, selectionText: "hi", aiEnabled: true }),
      acts,
    );
    items.find((i) => i.label === "Polish")!.action();
    expect(acts.runRewrite).toHaveBeenCalledWith("polish");
  });
});

describe("spelling group", () => {
  it("puts suggestions first so the fix is one click away", () => {
    const items = buildEditorMenuItems(context({ spelling: entry() }), actions());
    expect(labels(items).slice(0, 3)).toEqual(["the", "tech", "ten"]);
  });

  it("applies the suggestion the user picked", () => {
    const acts = actions();
    const e = entry();
    const items = buildEditorMenuItems(context({ spelling: e }), acts);
    items.find((i) => i.label === "tech")!.action();
    expect(acts.applySpelling).toHaveBeenCalledWith(e, "tech");
  });

  it("caps the suggestion list", () => {
    const many = entry({ suggestions: ["a", "b", "c", "d", "e", "f", "g"] });
    const items = buildEditorMenuItems(context({ spelling: many }), actions());
    const suggestionRows = items.filter((i) => many.suggestions.includes(i.label));
    expect(suggestionRows).toHaveLength(MAX_SUGGESTIONS);
  });

  it("offers a per-word dictionary entry, not just ignore-everything", () => {
    const acts = actions();
    const items = buildEditorMenuItems(context({ spelling: entry() }), acts);
    const add = items.find((i) => i.label.startsWith("Add "))!;
    expect(add.label).toContain("teh");
    add.action();
    expect(acts.addToDictionary).toHaveBeenCalledWith("teh");
  });

  it("says so when there is nothing to suggest", () => {
    const items = buildEditorMenuItems(
      context({ spelling: entry({ suggestions: [] }) }),
      actions(),
    );
    expect(items[0].label).toBe("No suggestions");
    expect(items[0].disabled).toBe(true);
  });
});

describe("link group", () => {
  it("offers open and copy on a link", () => {
    const items = buildEditorMenuItems(
      context({ link: { range: { from: 0, to: 5, kind: "url" }, text: "https://x.dev" } }),
      actions(),
    );
    expect(labels(items)).toEqual(expect.arrayContaining(["Open link", "Copy link"]));
  });

  it("is absent when the pointer is not on a link", () => {
    const items = buildEditorMenuItems(context(), actions());
    expect(labels(items)).not.toContain("Open link");
  });
});

describe("prompt and search rows", () => {
  it("offers placeholder filling only when the document has placeholders", () => {
    expect(labels(buildEditorMenuItems(context({ hasPlaceholders: true }), actions()))).toContain(
      "Fill placeholders…",
    );
    expect(labels(buildEditorMenuItems(context(), actions()))).not.toContain("Fill placeholders…");
  });

  it("echoes the selection in the search row and truncates a long one", () => {
    const long = "x".repeat(80);
    const items = buildEditorMenuItems(
      context({ hasSelection: true, selectionText: long }),
      actions(),
    );
    const row = items.find((i) => i.label.startsWith("Search workspace"))!;
    expect(row.label.length).toBeLessThan(60);
    expect(row.label).toContain("…");
  });
});

describe("helpers", () => {
  it("finds the entry covering a position and no other", () => {
    const entries = [entry({ from: 0, to: 3 }), entry({ from: 10, to: 15 })];
    expect(spellingAt(entries, 1)).toBe(entries[0]);
    expect(spellingAt(entries, 12)).toBe(entries[1]);
    // The end offset is exclusive: the caret after a word is not inside it.
    expect(spellingAt(entries, 3)).toBeNull();
    expect(spellingAt(entries, 7)).toBeNull();
  });

  it("collapses whitespace when truncating", () => {
    expect(truncate("a\n  b\tc", 40)).toBe("a b c");
    expect(truncate("abcdef", 3)).toBe("abc…");
  });
});
