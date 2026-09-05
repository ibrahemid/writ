import { describe, it, expect, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import type { Completion, CompletionContext } from "@codemirror/autocomplete";
import {
  wikilinkQueryAt,
  wikilinkCompletionSource,
  type NoteName,
} from "../../editor/wikilink-complete";

function contextAt(doc: string, pos: number, explicit = false): CompletionContext {
  return {
    state: EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] }),
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

const notes: NoteName[] = [
  { path: "/notes/Grocery list.md", name: "Grocery list" },
  { path: "/notes/a/Growth.md", name: "Growth" },
];

describe("wikilinkCompletionSource", () => {
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

describe("the code guard", () => {
  const source = wikilinkCompletionSource({ candidates: async () => notes });

  it.each([
    ["fenced", "```\n[[Gro\n```", 9],
    ["indented", "    [[Gro", 9],
    ["inline", "write `[[Gro` here", 12],
  ])("offers nothing inside %s code", async (_kind, doc, pos) => {
    expect(await source(contextAt(doc, pos))).toBeNull();
  });

  // An unmatched backtick is literal text in CommonMark, and the index reads
  // it that way too, so a link after one is a link.
  it("offers names after a backtick that opens nothing", async () => {
    expect(await source(contextAt("write ` then [[Gro", 18))).not.toBeNull();
  });
});

describe("the frontmatter guard", () => {
  const source = wikilinkCompletionSource({ candidates: async () => notes });

  it("offers nothing inside the block", async () => {
    expect(await source(contextAt('---\nsee: "[[Gro\n---\nbody', 15))).toBeNull();
  });

  it("offers names in the body under it", async () => {
    expect(await source(contextAt("---\na: 1\n---\n[[Gro", 18))).not.toBeNull();
  });

  // An unterminated block is body text, so what is written in it is prose.
  it("offers names under a rule that closes no block", async () => {
    expect(await source(contextAt('---\nsee: "[[Gro\nstill body', 15))).not.toBeNull();
  });
});

describe("accepting a name", () => {
  /** The document after the first option is applied at `pos`. */
  async function accept(doc: string, pos: number): Promise<string> {
    const source = wikilinkCompletionSource({ candidates: async () => notes });
    const context = contextAt(doc, pos);
    const result = await source(context);
    expect(result).not.toBeNull();
    const view = new EditorView({
      state: EditorState.create({
        doc,
        selection: { anchor: pos },
        extensions: [markdown({ base: markdownLanguage })],
      }),
      parent: document.body,
    });
    const option = result!.options[0] as Completion & {
      apply: (v: EditorView, c: Completion, f: number, t: number) => void;
    };
    option.apply(view, option, result!.from, result!.to ?? context.pos);
    const text = view.state.doc.toString();
    view.destroy();
    return text;
  }

  // `closeBrackets` wrote the pair when the `[[` was typed.
  it("writes the name into a link that is already closed", async () => {
    expect(await accept("[[Gro]]", 5)).toBe("[[Grocery list]]");
  });

  // A pasted or re-opened `[[` has no close, and a name without one is not a
  // link.
  it("closes a link that has no close of its own", async () => {
    expect(await accept("[[Gro", 5)).toBe("[[Grocery list]]");
  });
});
