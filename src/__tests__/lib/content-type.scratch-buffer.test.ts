import { describe, it, expect } from "vitest";
import { contentTypeForBuffer } from "../../lib/content-type";
import type { BufferDocument } from "../../types/buffer";

// Regression for the L2 escape: a scratch buffer whose user-renamed title
// ends in a renderable extension (e.g. "test.html") must resolve to that
// content type even though the Rust-generated filename is "<uuid>.txt".
// The previous chain — source_path ?? filename ?? title — picked the
// meaningless filename first and returned null, so the preview pane never
// mounted (PreviewLayout.showsPane stayed false). See #97 for the
// flash-on-switch follow-up surfaced by the same diagnostic.

function buf(overrides: Partial<BufferDocument>): BufferDocument {
  return {
    id: "B",
    title: "writ-1700000000",
    filename: "abc-uuid.txt",
    status: "active",
    language: null,
    source_path: null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "",
    updated_at: "",
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    ...overrides,
  };
}

describe("contentTypeForBuffer — scratch buffer renderability", () => {
  it("scratch buffer with .html title resolves to html", () => {
    expect(contentTypeForBuffer(buf({ title: "test.html" }))).toBe("html");
  });

  it("scratch buffer with .md title resolves to markdown", () => {
    expect(contentTypeForBuffer(buf({ title: "notes.md" }))).toBe("markdown");
  });

  it("default scratch (no extension in title, .txt filename) still returns null", () => {
    // The Rust create_buffer default: title "writ-<ms>", filename "<uuid>.txt".
    // Neither carries a renderable extension; the preview must stay source.
    expect(contentTypeForBuffer(buf({}))).toBeNull();
  });

  it("source-backed file's source_path wins over a contradictory title", () => {
    // A user renamed an on-disk Rust file's tab to "doc.html". The path is
    // authoritative — the file is still Rust, not HTML.
    expect(
      contentTypeForBuffer(
        buf({ source_path: "/proj/main.rs", title: "doc.html", filename: "main.rs" }),
      ),
    ).toBeNull();
  });

  it("renamed scratch (title has no extension) falls through to filename", () => {
    // If neither source_path nor title yields a recognized extension, the
    // filename is tried last. This case is rare in practice — scratch
    // filenames are <uuid>.txt — but the precedence must still be defined.
    expect(
      contentTypeForBuffer(buf({ title: "just-a-name", filename: "draft.md" })),
    ).toBe("markdown");
  });
});
