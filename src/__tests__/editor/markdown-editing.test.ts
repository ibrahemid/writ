import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { syntaxTree } from "@codemirror/language";
import { markdownEditingExtension } from "../../editor/markdown-editing";
import { registerBuiltinLanguages } from "../../editor/builtins";
import { getExtension } from "../../editor/language-registry";

describe("markdownEditingExtension runtime", () => {
  it("mounts in a view without throwing", () => {
    const state = EditorState.create({
      doc: "hello",
      extensions: [getExtension("markdown"), markdownEditingExtension],
    });
    const view = new EditorView({ state });
    expect(() => view.dispatch({ selection: { anchor: 2 } })).not.toThrow();
    view.destroy();
  });
});

describe("registered markdown language", () => {
  registerBuiltinLanguages();

  function nodeNames(doc: string): Set<string> {
    const state = EditorState.create({ doc, extensions: [getExtension("markdown")] });
    const names = new Set<string>();
    syntaxTree(state).iterate({
      enter: (node) => {
        names.add(node.name);
      },
    });
    return names;
  }

  it("parses strikethrough", () => {
    expect(nodeNames("~~gone~~\n")).toContain("Strikethrough");
  });

  it("parses task list markers", () => {
    const names = nodeNames("- [ ] open\n- [x] done\n");
    expect(names).toContain("Task");
    expect(names).toContain("TaskMarker");
  });

  it("parses autolinks", () => {
    expect(nodeNames("visit https://writ.dev today\n")).toContain("URL");
  });
});
