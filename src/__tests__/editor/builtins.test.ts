import { describe, it, expect, beforeEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { ensureSyntaxTree } from "@codemirror/language";
import { LanguageDescription } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { registerBuiltinLanguages } from "../../editor/builtins";
import { getExtension, listLanguageIds, unregisterAll } from "../../editor/language-registry";

const PARSE_TIMEOUT_MS = 30_000;

// The nested language is mounted over the fence's CodeText, and a mount is
// reached by resolving a position rather than by iterating the outer tree,
// which is why the markdown decoration builder never sees inside a block.
function nodeAt(doc: string, pos: number): { name: string; parent: string | null } {
  const state = EditorState.create({ doc, extensions: [getExtension("markdown")] });
  const tree = ensureSyntaxTree(state, doc.length, PARSE_TIMEOUT_MS);
  const node = tree!.resolveInner(pos, 1);
  return { name: node.name, parent: node.parent?.name ?? null };
}

describe("registerBuiltinLanguages", () => {
  beforeEach(() => {
    unregisterAll();
    registerBuiltinLanguages();
  });

  it("registers markdown with the code-language table so a fenced block highlights", async () => {
    // The table loads a language the first time an info string names it, so
    // the test waits for the same load the editor does. The nested language
    // then parses into the one markdown tree, which is what the highlight
    // style paints from: no second markdown parser is involved.
    const named = LanguageDescription.matchLanguageName(languages, "ts", true);
    expect(named).not.toBeNull();
    await named!.load();
    const doc = "intro\n```ts\nlet a = 1\n```\n";
    expect(nodeAt(doc, doc.indexOf("let") + 1)).toEqual({
      name: "let",
      parent: "VariableDeclaration",
    });
  });

  it("leaves a fence with an unknown info string as plain code text", () => {
    const doc = "intro\n```nosuchlanguage\nlet a = 1\n```\n";
    expect(nodeAt(doc, doc.indexOf("let") + 1)).toEqual({
      name: "CodeText",
      parent: "FencedCode",
    });
  });

  it("keeps every language the editor offers", () => {
    expect(listLanguageIds()).toEqual([
      "javascript",
      "typescript",
      "python",
      "rust",
      "json",
      "html",
      "css",
      "markdown",
      "php",
      "sql",
    ]);
  });
});
