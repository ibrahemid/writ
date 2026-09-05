import { describe, it, expect } from "vitest";
import { wikilinkName } from "../../lib/wikilink";

// The Rust side of this split is `writ_core::notes::links::parse_wikilink`.
// The two have to agree: the name is what `Create note` gives the file, so a
// disagreement mints a note under a name no link points at.
describe("wikilinkName", () => {
  it.each([
    ["Note", "Note"],
    ["Note|alias", "Note"],
    ["Note#Heading", "Note"],
    ["folder/Note", "Note"],
    ["folder/Note#Heading|alias", "Note"],
    ["Note.md", "Note"],
    ["Note.markdown", "Note"],
    ["Note.MD", "Note"],
    ["folder/Note.md", "Note"],
    ["Note.md.md", "Note.md"],
    ["a.b.md", "a.b"],
    ["Note.txt", "Note.txt"],
    ["a/b/c/Deep", "Deep"],
    ["folder\\Note", "Note"],
    ["  Padded  ", "Padded"],
  ])("reads %s as %s", (target, name) => {
    expect(wikilinkName(target)).toBe(name);
  });

  // An empty segment is not a folder, so a trailing separator leaves the name
  // before it, the way parse_target reads it.
  it("drops empty path segments", () => {
    expect(wikilinkName("folder/")).toBe("folder");
    expect(wikilinkName("a/b/")).toBe("b");
    expect(wikilinkName("a//b")).toBe("b");
  });

  it("reads a target with no name as empty", () => {
    expect(wikilinkName("")).toBe("");
    expect(wikilinkName(".md")).toBe("");
    expect(wikilinkName("folder/.md")).toBe("");
    expect(wikilinkName("#Heading")).toBe("");
    expect(wikilinkName("|alias")).toBe("");
  });

  // The alias comes first, so a `#` or a `/` written inside one is text.
  it("takes the alias before the heading and the folder", () => {
    expect(wikilinkName("Note|a#b")).toBe("Note");
    expect(wikilinkName("Note|a/b")).toBe("Note");
  });
});
