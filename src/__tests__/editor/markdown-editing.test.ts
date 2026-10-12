import { describe, it, expect, beforeAll } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { LanguageSupport } from "@codemirror/language";
import { markdownEditingExtension } from "../../editor/markdown-editing";
import { loadGrammar } from "../../editor/languages";

let markdown: LanguageSupport;

beforeAll(async () => {
  const support = await loadGrammar("markdown");
  if (!support) throw new Error("no Markdown grammar");
  markdown = support;
});

describe("markdownEditingExtension runtime", () => {
  it("mounts in a view without throwing", () => {
    const state = EditorState.create({
      doc: "hello",
      extensions: [markdown, markdownEditingExtension],
    });
    const view = new EditorView({ state });
    expect(() => view.dispatch({ selection: { anchor: 2 } })).not.toThrow();
    view.destroy();
  });
});
