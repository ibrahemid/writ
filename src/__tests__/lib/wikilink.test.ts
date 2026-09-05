import { describe, it, expect } from "vitest";
import {
  noteLinkHeading,
  wikilinkFileName,
  wikilinkName,
  wikilinkTargetPath,
} from "../../lib/wikilink";

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

// What `Create note` sends. Rust removes exactly one note extension before it
// mints the file name, so this leaves the extension the link was written with
// on: taking it off here as well made `Note.md` out of `[[Note.markdown.md]]`,
// which is not the note that target resolves to.
describe("wikilinkFileName", () => {
  it.each([
    ["Note", "Note"],
    ["Note.md", "Note.md"],
    ["Note.markdown.md", "Note.markdown.md"],
    ["Note.md.md", "Note.md.md"],
    ["Note.txt", "Note.txt"],
    ["folder/Note.md", "Note.md"],
    ["Note.md#Heading|alias", "Note.md"],
    ["  Padded.md  ", "Padded.md"],
  ])("sends %s as %s", (target, sent) => {
    expect(wikilinkFileName(target)).toBe(sent);
  });

  it("is the name with whatever extension the link carried", () => {
    for (const target of ["Note", "Note.md", "Note.markdown.md", "a.b.md"]) {
      expect(wikilinkName(target)).toBe(
        wikilinkFileName(target).replace(/\.(md|markdown)$/i, ""),
      );
    }
  });
});

// The renderer writes a resolved `[[Note#Section]]` as a note href whose
// fragment is the heading's anchor.
describe("noteLinkHeading", () => {
  it.each([
    ["writ-note:Note.md#later-part", "later-part"],
    ["writ-note:Note.md#caf%C3%A9", "café"],
    ["writ-note:folder/Note.md#a-b", "a-b"],
    ["writ-note:a%23b.md#h", "h"],
    ["writ-note:Note.md", null],
    ["writ-note:Note.md#", null],
    ["https://example.com/x#h", null],
  ])("reads %s as %s", (href, heading) => {
    expect(noteLinkHeading(href)).toBe(heading);
  });

  it("hands back an escape it cannot decode rather than throwing", () => {
    expect(noteLinkHeading("writ-note:Note.md#%E0%A4%A")).toBe("%E0%A4%A");
  });
});

// What `Create note` sends. The folder travels with the name because a target
// carrying a `/` only resolves to a note whose own folders end the same way;
// Rust sanitises every segment before anything is created.
describe("wikilinkTargetPath", () => {
  it.each([
    ["Note", "Note"],
    ["projects/Ideas", "projects/Ideas"],
    ["a/b/Note.md", "a/b/Note.md"],
    ["a\\b\\Note", "a/b/Note"],
    ["../ideas/Note", "ideas/Note"],
    ["./Note", "Note"],
    ["projects/Note.markdown.md", "projects/Note.markdown.md"],
    ["  projects / Ideas.md  ", "projects/Ideas.md"],
    ["projects/Ideas#Section|alias", "projects/Ideas"],
  ])("sends %s as %s", (target, sent) => {
    expect(wikilinkTargetPath(target)).toBe(sent);
  });

  it("ends with the file name the same split gives", () => {
    for (const target of ["Note", "a/b/Note.md", "../ideas/Note"]) {
      expect(wikilinkTargetPath(target).split("/").pop()).toBe(wikilinkFileName(target));
    }
  });
});
