import { describe, it, expect, vi, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { LanguageSupport, ensureSyntaxTree } from "@codemirror/language";
import { DETECTABLE_LANGUAGE_IDS } from "../../services/language-detect";
import {
  LANGUAGE_CATALOG,
  LanguageCatalogError,
  PLAIN_TEXT_LANGUAGE,
  grammarFor,
  isKnownLanguage,
  languageChoices,
  languageForFileName,
  languageId,
  languageName,
  loadGrammar,
  loadedGrammar,
  warmGrammar,
} from "../../editor/languages";

// A fresh EditorState parses only within a short budget and keeps whatever tree
// it reached, so a parse is forced to the end of the document before a node is
// looked up.
const PARSE_TIMEOUT_MS = 30_000;

// A grammar's first load imports its module, which on a busy runner can take
// longer than the unit project's default budget.
const LOAD_BUDGET = { timeout: 30_000 };

async function tree(id: string, doc: string) {
  const support = await loadGrammar(id);
  if (!support) throw new Error(`no grammar for ${id}`);
  const state = EditorState.create({ doc, extensions: [support] });
  const parsed = ensureSyntaxTree(state, doc.length, PARSE_TIMEOUT_MS);
  if (!parsed) throw new Error(`${id} did not parse in time`);
  return parsed;
}

async function nodeNames(id: string, doc: string): Promise<Set<string>> {
  const names = new Set<string>();
  (await tree(id, doc)).iterate({
    enter: (node) => {
      names.add(node.name);
    },
  });
  return names;
}

async function nodeAt(id: string, doc: string, pos: number) {
  const node = (await tree(id, doc)).resolveInner(pos, 1);
  return { name: node.name, parent: node.parent?.name ?? null };
}

describe("every language the detector names has a grammar", LOAD_BUDGET, () => {
  it.each(DETECTABLE_LANGUAGE_IDS.map((id) => [id]))("%s", async (id) => {
    expect(grammarFor(id), `${id} names no grammar`).not.toBeNull();
    const support = await loadGrammar(id);
    expect(support).toBeInstanceOf(LanguageSupport);
    expect(loadedGrammar(id)).toBe(support);
  });
});

describe("the catalog", LOAD_BUDGET, () => {
  it("finds every entry by its own id", () => {
    for (const description of LANGUAGE_CATALOG) {
      expect(grammarFor(languageId(description))).toBe(description);
    }
  });

  it("holds each language once", () => {
    const ids = LANGUAGE_CATALOG.map(languageId);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("finds a language by an alias", () => {
    expect(grammarFor("bash")?.name).toBe("Shell");
    expect(grammarFor("ts")?.name).toBe("TypeScript");
  });

  it("has no grammar for plain text, no choice or an unknown id", async () => {
    expect(grammarFor(PLAIN_TEXT_LANGUAGE)).toBeNull();
    expect(grammarFor(null)).toBeNull();
    expect(grammarFor("no-such-language")).toBeNull();
    expect(isKnownLanguage(PLAIN_TEXT_LANGUAGE)).toBe(false);
    expect(isKnownLanguage("python")).toBe(true);
    expect(await loadGrammar(PLAIN_TEXT_LANGUAGE)).toBeNull();
    expect(await loadGrammar(null)).toBeNull();
    expect(loadedGrammar("no-such-language")).toBeNull();
  });

  it("loads a grammar once", async () => {
    const first = await loadGrammar("toml");
    expect(await loadGrammar("toml")).toBe(first);
  });
});

describe("languageForFileName", () => {
  it("answers an extension Writ's own map does not list", () => {
    expect(languageForFileName("main.c")).toBe("c");
    expect(languageForFileName("Build.kt")).toBe("kotlin");
  });

  it("answers a whole name", () => {
    expect(languageForFileName("Dockerfile")).toBe("dockerfile");
  });

  it("answers null for a name the table does not know", () => {
    expect(languageForFileName("notes.nosuchext")).toBeNull();
    expect(languageForFileName("LICENSE")).toBeNull();
  });

  // The table lists `text` under LaTeX.
  it("answers null for a name that says plain text, in any case", () => {
    for (const name of ["meeting.text", "meeting.TEXT", "notes.txt", "NOTES.TXT"]) {
      expect(languageForFileName(name), name).toBeNull();
    }
  });

  it("answers a whole name the table knows, unless the name says plain text", () => {
    expect(languageForFileName("PKGBUILD")).toBe("shell");
    expect(languageForFileName("nginx.conf")).toBe("nginx");
    expect(languageForFileName("Jenkinsfile")).toBe("groovy");
    expect(languageForFileName("CMakeLists.txt")).toBeNull();
  });

  it("reads an extension in any case", () => {
    expect(languageForFileName("MAIN.C")).toBe("c");
    expect(languageForFileName("Build.KT")).toBe("kotlin");
    expect(languageForFileName("analysis.R")).toBe("r");
  });

  it("answers null for the table's TTCN and properties readings of .cfg and .in", () => {
    for (const name of ["setup.cfg", "haproxy.cfg", "Makefile.in", "config.h.in"]) {
      expect(languageForFileName(name), name).toBeNull();
    }
  });

  it("answers null for an extension more than one language in the table claims", () => {
    for (const name of ["main.m", "top.v", "release.sig"]) {
      expect(languageForFileName(name), name).toBeNull();
    }
  });
});

describe("languageChoices and languageName", () => {
  it("lists every catalog entry by name, in name order", () => {
    const choices = languageChoices();
    expect(choices).toHaveLength(LANGUAGE_CATALOG.length);
    const names = choices.map((choice) => choice.name);
    const sorted = [...names].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));
    expect(names).toEqual(sorted);
    expect(choices.find((choice) => choice.id === "python")).toEqual({
      id: "python",
      name: "Python",
      aliases: grammarFor("python")!.alias,
    });
  });

  it("names a known id and nothing else", () => {
    expect(languageName("python")).toBe("Python");
    expect(languageName("c++")).toBe("C++");
    expect(languageName("no-such-language")).toBeNull();
  });
});

describe("Writ's own grammars", LOAD_BUDGET, () => {
  it("parses JSX in a .js file", async () => {
    const names = await nodeNames("javascript", "const a = <div>hi</div>;\n");
    expect(names).toContain("JSXElement");
  });

  it("parses JSX in a .tsx file and not in a .ts file", async () => {
    expect(await nodeNames("tsx", "const a = <div>hi</div>;\n")).toContain("JSXElement");
    expect(await nodeNames("typescript", "const a = <T>value;\n")).not.toContain("JSXElement");
  });

  it("parses Markdown with the GitHub extensions", async () => {
    expect(await nodeNames("markdown", "~~gone~~\n")).toContain("Strikethrough");
    const tasks = await nodeNames("markdown", "- [ ] open\n- [x] done\n");
    expect(tasks).toContain("Task");
    expect(tasks).toContain("TaskMarker");
    expect(await nodeNames("markdown", "visit https://writ.dev today\n")).toContain("URL");
  });

  it("highlights a fenced block in the language its info string names", async () => {
    // The fence's grammar loads the first time an info string names it, so the
    // test waits for the same load the editor does.
    await loadGrammar("typescript");
    const doc = "intro\n```ts\nlet a = 1\n```\n";
    expect(await nodeAt("markdown", doc, doc.indexOf("let") + 1)).toEqual({
      name: "let",
      parent: "VariableDeclaration",
    });
  });

  it("leaves a fence with an unknown info string as code text", async () => {
    const doc = "intro\n```nosuchlanguage\nlet a = 1\n```\n";
    expect(await nodeAt("markdown", doc, doc.indexOf("let") + 1)).toEqual({
      name: "CodeText",
      parent: "FencedCode",
    });
  });
});

describe("warmGrammar", LOAD_BUDGET, () => {
  it("loads a grammar while the main thread is idle", async () => {
    expect(loadedGrammar("lua")).toBeNull();
    warmGrammar("lua");
    await vi.waitFor(() => expect(loadedGrammar("lua")).toBeInstanceOf(LanguageSupport), {
      timeout: 25_000,
      interval: 50,
    });
  });
});

describe("a catalog that lost a language Writ describes itself", () => {
  afterEach(() => {
    vi.doUnmock("@codemirror/language-data");
    vi.resetModules();
  });

  it("fails loudly instead of dropping the language", async () => {
    vi.resetModules();
    vi.doMock("@codemirror/language-data", () => ({ languages: [] }));
    await expect(import("../../editor/languages")).rejects.toBeInstanceOf(Error);
    await expect(import("../../editor/languages")).rejects.toThrow(
      "@codemirror/language-data no longer describes Markdown",
    );
    expect(new LanguageCatalogError("Markdown").name).toBe("LanguageCatalogError");
  });
});
