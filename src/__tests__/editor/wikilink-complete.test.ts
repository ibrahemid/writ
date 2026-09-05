import { describe, it, expect, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import type { CompletionContext } from "@codemirror/autocomplete";
import {
  wikilinkQueryAt,
  wikilinkCompletionSource,
  type NoteName,
} from "../../editor/wikilink-complete";

function contextAt(doc: string, pos: number, explicit = false): CompletionContext {
  return {
    state: EditorState.create({ doc }),
    pos,
    explicit,
  } as CompletionContext;
}

describe("wikilinkQueryAt", () => {
  it("reads what has been typed after an open bracket pair", () => {
    expect(wikilinkQueryAt("see [[Gro", 9)).toBe("Gro");
    expect(wikilinkQueryAt("[[", 2)).toBe("");
  });

  it("is null outside a wikilink and after a closed one", () => {
    expect(wikilinkQueryAt("plain text", 5)).toBeNull();
    expect(wikilinkQueryAt("[[Note]] and more", 17)).toBeNull();
    expect(wikilinkQueryAt("[[a[b", 5)).toBeNull();
  });

  // `closeBrackets` turns a typed `[[` into `[[]]` with the caret between the
  // pairs, so the query is read from the text before the caret alone.
  it("reads a query with the closing pair already there", () => {
    expect(wikilinkQueryAt("[[Gro]]", 5)).toBe("Gro");
  });
});

describe("wikilinkCompletionSource", () => {
  const notes: NoteName[] = [
    { path: "/notes/Grocery list.md", name: "Grocery list" },
    { path: "/notes/a/Growth.md", name: "Growth" },
  ];

  it("offers the names the index ranked, in that order", async () => {
    const candidates = vi.fn(async () => notes);
    const source = wikilinkCompletionSource({ candidates });
    const result = await source(contextAt("[[Gro]]", 5));

    expect(candidates).toHaveBeenCalledWith("Gro");
    expect(result?.from).toBe(2);
    expect(result?.to).toBe(5);
    expect(result?.filter).toBe(false);
    expect(result?.options.map((o) => o.label)).toEqual(["Grocery list", "Growth"]);
    expect(result?.options[0].detail).toBe("/notes/Grocery list.md");
  });

  it("offers nothing outside a wikilink", async () => {
    const candidates = vi.fn(async () => notes);
    const source = wikilinkCompletionSource({ candidates });
    expect(await source(contextAt("plain text", 5))).toBeNull();
    expect(candidates).not.toHaveBeenCalled();
  });

  it("waits for a character before opening the list on its own", async () => {
    const candidates = vi.fn(async () => notes);
    const source = wikilinkCompletionSource({ candidates });
    expect(await source(contextAt("[[]]", 2))).toBeNull();
    expect(candidates).not.toHaveBeenCalled();

    expect(await source(contextAt("[[]]", 2, true))).not.toBeNull();
  });

  it("offers nothing when the index knows no such name", async () => {
    const source = wikilinkCompletionSource({ candidates: async () => [] });
    expect(await source(contextAt("[[zzz]]", 5))).toBeNull();
  });
});
