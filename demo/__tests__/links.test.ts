import { describe, expect, it } from "vitest";
import {
  listCandidateNameKeys,
  extractHeadings,
  extractProperties,
  extractTags,
  slugifyHeading,
  findNameSpan,
  getNoteDisplayName,
  parseTarget,
  parseWikilink,
  resolveTarget,
  rewriteLinks,
  scanLinks,
  findSentenceAt,
  splitFrontmatter,
  parseStoredTarget,
} from "../backend/links";

// Vectors ported from the Rust test modules of writ_core::notes::{links, facts,
// snippet, rename}, so the page reads a note the way the index does.

const targets = (text: string) => scanLinks(text).map((l) => l.target);
const tagNames = (text: string) => extractTags(text).map(([tag]) => tag);

describe("scanLinks", () => {
  it("scans a bare wikilink with its position", () => {
    const text = "see [[Note]] here\n";
    const [link] = scanLinks(text);
    expect(link).toMatchObject({ kind: "wikilink", target: "Note", alias: null, heading: null, line: 1, col: 4 });
    expect(text.slice(...link.range)).toBe("[[Note]]");
  });

  it("splits the alias and the heading off the target", () => {
    expect(scanLinks("[[Note|the label]]\n")[0]).toMatchObject({ target: "Note", alias: "the label" });
    expect(scanLinks("[[Note#Some Heading]]\n")[0]).toMatchObject({ target: "Note", heading: "Some Heading" });
  });

  it("names the note a stored link names the way the written one does", () => {
    for (const inner of ["Note", "Note.md", "Note.md.md", "Note.markdown.md", "a.b.md", "list.txt", "folder/Note.md", "folder\\Note.md.md", "Note.md#Heading", "Note.md|alias"]) {
      const [link] = scanLinks(`[[${inner}]]\n`);
      const stored = parseStoredTarget(link.target);
      const written = parseWikilink(inner);
      expect([stored.name, stored.folder, link.heading, link.alias]).toEqual([written.name, written.folder, written.heading, written.alias]);
    }
  });

  it("reads markdown links that name a note and nothing else", () => {
    expect(scanLinks("[label](./x.md)\n")[0]).toMatchObject({ kind: "markdown", target: "x" });
    expect(targets("[label](sub/other)\n")).toEqual(["sub/other"]);
    for (const text of ["[a](https://example.com)\n", "[a](mailto:x@example.com)\n", "![a](picture.png)\n", "[a](#section)\n", "[a](sheet.csv)\n"]) {
      expect(targets(text)).toEqual([]);
    }
    expect(scanLinks("[a](notes/x.md#the-part)\n")[0]).toMatchObject({ target: "notes/x", heading: "the-part" });
    expect(targets("[a](My%20Note.md)\n")).toEqual(["My Note"]);
  });

  it("skips fences, inline code and the frontmatter", () => {
    expect(targets("```\n[[Note]]\n```\n")).toEqual([]);
    expect(targets("~~~md\n[[Note]]\n~~~\n")).toEqual([]);
    expect(targets("write `[[Note]]` to link\n")).toEqual([]);
    expect(targets("`code` then [[Note]]\n")).toEqual(["Note"]);
    expect(targets("---\nsee: \"[[Note]]\"\n---\nbody\n")).toEqual([]);
    expect(targets("---\nt: x\n---\n[[Note]]\n")).toEqual(["Note"]);
    expect(targets("[[Note\n")).toEqual([]);
  });

  it("counts columns in characters", () => {
    const text = "first\nBefore [[Note]]\n";
    const [link] = scanLinks(text);
    expect([link.line, link.col]).toEqual([2, 7]);
    expect(scanLinks("مرحبا [[Note]]\n")[0].col).toBe(6);
  });
});

describe("parsing and resolving a target", () => {
  it("takes the alias first, then the heading", () => {
    expect(parseWikilink("Note#Heading|Label")).toMatchObject({ name: "Note", heading: "Heading", alias: "Label" });
    expect(parseWikilink("Note|Label#not a heading")).toMatchObject({ name: "Note", heading: null, alias: "Label#not a heading" });
    expect(parseTarget("a/b/Note.md")).toMatchObject({ folder: "a/b", name: "Note" });
    expect(parseTarget("Note.txt").name).toBe("Note.txt");
  });

  it("ranks by depth, then by the nearer ancestor, and never guesses a tie", () => {
    expect(resolveTarget(parseTarget("Note"), "/n/From.md", ["/n/Other.md"])).toEqual({ status: "missing" });
    expect(resolveTarget(parseTarget("Note"), "/n/deep/From.md", ["/n/deep/inner/Note.md", "/n/Note.md"])).toEqual({ status: "resolved", path: "/n/Note.md" });
    const two = ["/n/a/Note.md", "/n/b/Note.md"];
    expect(resolveTarget(parseTarget("Note"), "/n/b/From.md", two)).toEqual({ status: "resolved", path: "/n/b/Note.md" });
    expect(resolveTarget(parseTarget("Note"), "/n/c/From.md", two)).toEqual({ status: "ambiguous", candidates: two });
    expect(resolveTarget(parseTarget("b/Note"), "/n/c/From.md", two)).toEqual({ status: "resolved", path: "/n/b/Note.md" });
  });

  it("folds case and unicode normalisation", () => {
    expect(resolveTarget(parseTarget("weekly review"), "/n/From.md", ["/n/Weekly Review.md"])).toEqual({ status: "resolved", path: "/n/Weekly Review.md" });
    expect(resolveTarget(parseTarget("Café"), "/n/From.md", ["/n/Café.md"])).toEqual({ status: "resolved", path: "/n/Café.md" });
    expect(listCandidateNameKeys("/n/Note.md")).toEqual(["note", "note.md"]);
    expect(listCandidateNameKeys("/n/list.txt")).toEqual(["list.txt"]);
  });
});

describe("frontmatter", () => {
  it("splits the block from the body, and leaves an unterminated one as body", () => {
    expect(splitFrontmatter("---\na: 1\n---\nbody\n")).toEqual(["---\na: 1\n---\n", "body\n"]);
    expect(splitFrontmatter("---\na: 1\nbody\n")).toEqual([null, "---\na: 1\nbody\n"]);
  });

  it("reads scalars and sequences as the JSON serde_json writes", () => {
    expect(extractProperties("---\ntitle: Weekly review\ndone: true\ncount: 3\ntags: [a, b]\n---\n")).toEqual([
      ["title", '"Weekly review"'],
      ["done", "true"],
      ["count", "3"],
      ["tags", '["a","b"]'],
    ]);
    expect(extractProperties("---\ntags:\n  - one\n  - two\n---\n")).toEqual([["tags", '["one","two"]']]);
    expect(extractProperties('---\ntitle: "a: b"\n---\n')).toEqual([["title", '"a: b"']]);
    expect(extractProperties("---\nmeta:\n  a: 1\n  b: 2\n---\n")).toEqual([["meta", '"  a: 1\\n  b: 2"']]);
    expect(extractProperties("---\nnote:\n---\n")).toEqual([["note", "null"]]);
    expect(extractProperties("# Heading\n\nbody\n")).toEqual([]);
  });

  it("writes a float the way serde_json does and keeps block scalars", () => {
    expect(extractProperties("---\na: 1.5\nb: 3.0\nc: 0x10\n---\n")).toEqual([["a", "1.5"], ["b", "3.0"], ["c", '"0x10"']]);
    expect(extractProperties("---\nsummary: >\n  one\n  two\n---\n")).toEqual([["summary", '"one two"']]);
  });
});

describe("tags", () => {
  it("finds body tags and leaves urls, numbers, headings and code alone", () => {
    expect(extractTags("#inbox and some #draft/two text\n")).toEqual([["inbox", 1], ["draft/two", 1]]);
    expect(extractTags("see https://example.com/x#section for more\n")).toEqual([]);
    expect(extractTags("issue #123 and\n")).toEqual([]);
    expect(extractTags("# Heading\n")).toEqual([]);
    expect(extractTags("```\n#reading\n```\n")).toEqual([]);
    expect(extractTags("write `#reading` for that\n")).toEqual([]);
  });

  it("tells a colour from a word", () => {
    const css = "  .card{border:1px solid #D9DEE8;background:#fff}\n--ink: #16161e; --card: #fff;\n<path fill=\"#7A8095\"/>\nBackground #fafafa and border #E3E6EE\nPrimary: #0a7d4f\n#fff #eee #ffffff #ababab #abcabc #deadbeef;\n";
    expect(extractTags(css)).toEqual([]);
    expect(tagNames("#cafe #dead #abc #b2b #e2e #facade\n")).toEqual(["cafe", "dead", "abc", "b2b", "e2e", "facade"]);
    expect(extractTags("---\ntags: [fff, 0a7d4f]\n---\n")).toEqual([["fff", 2], ["0a7d4f", 2]]);
  });

  it("skips anchors in pasted markup and opens in prose brackets", () => {
    const markup = "<a href=\"#top\">up</a> <a href='#faq'>faq</a>\n<path marker-end=\"url(#aA)\" d=\"M0,0\"/>\n[contents](#section) and f(#x)\n";
    expect(extractTags(markup)).toEqual([]);
    expect(tagNames("filed (#work) and \"#home\" and '#errand'\n")).toEqual(["work", "home", "errand"]);
  });

  it("reads frontmatter tags with their lines", () => {
    expect(extractTags("---\ntitle: x\n#reading: y\n---\nbody\n")).toEqual([]);
    expect(extractTags("---\ntitle: One\ntags: [alpha, project/beta]\n---\n\n#gamma\n")).toEqual([["alpha", 3], ["project/beta", 3], ["gamma", 6]]);
    expect(extractTags('---\ntag: alpha\ntags:\n  - beta\n  - "#project/gamma"\n---\nbody\n')).toEqual([["alpha", 2], ["beta", 4], ["project/gamma", 5]]);
    expect(extractTags('---\ntags: [2026, ""]\ntopics: [alpha]\n---\nbody\n')).toEqual([]);
    expect(extractTags("---\ncover:\n  tags: [alpha]\n---\nbody\n")).toEqual([]);
    expect(extractTags("---\ntags: [two words]\n---\nbody\n")).toEqual([]);
  });

  it("splits a value into the tags it names and drops a comment", () => {
    for (const text of ["---\ntags: work, urgent\n---\nbody\n", '---\ntags: "work, urgent"\n---\nbody\n', "---\ntags: work urgent\n---\nbody\n", "---\ntag: work, urgent\n---\nbody\n"]) {
      expect(extractTags(text)).toEqual([["work", 2], ["urgent", 2]]);
    }
    expect(extractTags("---\ntags:\n  - work, urgent\n---\nbody\n")).toEqual([["work", 3], ["urgent", 3]]);
    expect(extractTags("---\ntags: [alpha, beta] # sort these\n---\nbody\n")).toEqual([["alpha", 2], ["beta", 2]]);
    expect(extractTags("---\ntags: work # mine later\n---\nbody\n")).toEqual([["work", 2]]);
    expect(extractTags("---\ntags: #work #urgent\n---\nbody\n")).toEqual([["work", 2], ["urgent", 2]]);
    expect(extractTags("---\ntags: # the ones below\n  - work\n---\nbody\n")).toEqual([["work", 3]]);
    expect(extractTags("---\ntags: [Project]\n---\n\n#project and #PROJECT\n")).toEqual([["project", 2], ["project", 5], ["project", 5]]);
  });
});

describe("headings", () => {
  it("carries level, line and slug, and disambiguates a repeat", () => {
    expect(extractHeadings("# One\n\ntext\n\n### Two Words\n")).toEqual([
      { level: 1, text: "One", line: 1, slug: "one" },
      { level: 3, text: "Two Words", line: 5, slug: "two-words" },
    ]);
    expect(extractHeadings("# Notes\n# Notes\n# Notes\n").map((h) => h.slug)).toEqual(["notes", "notes-1", "notes-2"]);
    expect(extractHeadings("## Title ##\n")[0].text).toBe("Title");
    expect(extractHeadings("```\n# Not a heading\n```\n")).toEqual([]);
  });

  it("slugs the way GitHub does", () => {
    expect(slugifyHeading("Some Heading")).toBe("some-heading");
    expect(slugifyHeading("What's next?")).toBe("whats-next");
    expect(slugifyHeading("Café أهلا")).toBe("café-أهلا");
  });
});

describe("findSentenceAt", () => {
  const at = (text: string) => findSentenceAt(text, text.indexOf("[["));

  it("takes the sentence around the link and never crosses a line", () => {
    expect(at("First one. Second holds [[Note]] here. Third one.")).toBe("Second holds [[Note]] here.");
    expect(at("Holds [[Note]] here. Second one.")).toBe("Holds [[Note]] here.");
    expect(at("First one. Trailing [[Note]]")).toBe("Trailing [[Note]]");
    expect(at("# Heading\nBody with [[Note]]\nNext line.\n")).toBe("Body with [[Note]]");
    expect(at("See [[Note.md]] and version 1.5 of it.")).toBe("See [[Note.md]] and version 1.5 of it.");
    expect(at("Really?! Then [[Note]] said so.")).toBe("Then [[Note]] said so.");
    expect(at("أين الملف؟ يوجد في [[Note]] هنا.")).toBe("يوجد في [[Note]] هنا.");
  });

  it("windows a long sentence around the link", () => {
    const padding = "x".repeat(320);
    const long = at(`${padding} [[Note]] ${padding}`);
    expect([...long].length).toBe(320);
    expect(long).toContain("[[Note]]");
    const tail = at(`${"x".repeat(640)} [[Note]]`);
    expect(tail.endsWith("[[Note]]")).toBe(true);
    expect(findSentenceAt("", 0)).toBe("");
    expect(findSentenceAt("First line.\nLast line.", 999)).toBe("Last line.");
  });
});

describe("rewriteLinks", () => {
  const READER = "/notes/Reader.md";
  const rewrite = (text: string, target: string, name: string) => rewriteLinks(text, READER, target, name, [READER, target]);

  it("renames the note and keeps the rest of each link", () => {
    expect(rewrite("see [[Old note]] for more", "/notes/Old note.md", "New note")).toBe("see [[New note]] for more");
    expect(rewrite("[[Old note|what I meant]]", "/notes/Old note.md", "New note")).toBe("[[New note|what I meant]]");
    expect(rewrite("[[Old note#Later on|see this]]", "/notes/Old note.md", "New note")).toBe("[[New note#Later on|see this]]");
    expect(rewrite("[[ideas/Old note]]", "/notes/ideas/Old note.md", "New note")).toBe("[[ideas/New note]]");
    expect(rewrite("[[Old note.md]]", "/notes/Old note.md", "New note")).toBe("[[New note.md]]");
    expect(rewrite("[what I wrote](ideas/Old%20note.md)", "/notes/ideas/Old note.md", "New note")).toBe("[what I wrote](ideas/New%20note.md)");
    expect(rewrite("[a](<Old note.md>)", "/notes/Old note.md", "New note")).toBe("[a](<New note.md>)");
    expect(rewrite("nothing here", "/notes/Old note.md", "New note")).toBeNull();
  });

  it("locates the name inside one link", () => {
    expect(findNameSpan("[[a/Note.md#h|x]]")?.range).toEqual([4, 8]);
    expect(findNameSpan("[l](a/My%20Note.md)")).toEqual({ range: [6, 15], escaping: "percent" });
  });

  it("names a note by its file name without a note extension", () => {
    expect(getNoteDisplayName("/n/Plan.md")).toBe("Plan");
    expect(getNoteDisplayName("/n/list.txt")).toBe("list.txt");
  });
});
