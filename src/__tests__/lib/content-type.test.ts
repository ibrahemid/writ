import { describe, it, expect } from "vitest";
import { contentTypeForBuffer } from "../../lib/content-type";
import type { BufferDocument } from "../../types/buffer";

function buf(overrides: Partial<BufferDocument>): BufferDocument {
  return {
    id: "b",
    title: "untitled",
    filename: "untitled",
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

describe("contentTypeForBuffer", () => {
  it(".html and .htm map to html", () => {
    expect(contentTypeForBuffer(buf({ filename: "page.html" }))).toBe("html");
    expect(contentTypeForBuffer(buf({ filename: "page.HTM" }))).toBe("html");
  });

  it(".md and friends map to markdown", () => {
    for (const ext of ["md", "markdown", "mdown", "mkd"]) {
      expect(contentTypeForBuffer(buf({ filename: `n.${ext}` }))).toBe("markdown");
    }
  });

  it(".mmd and .mermaid map to mermaid", () => {
    expect(contentTypeForBuffer(buf({ filename: "flow.mmd" }))).toBe("mermaid");
    expect(contentTypeForBuffer(buf({ filename: "flow.MERMAID" }))).toBe("mermaid");
  });

  it("prefers source_path over filename when both end differently", () => {
    expect(
      contentTypeForBuffer(buf({ source_path: "/a/notes.md", filename: "notes" })),
    ).toBe("markdown");
  });

  it("returns null for unknown / extension-less buffers", () => {
    expect(contentTypeForBuffer(buf({ filename: "main.rs" }))).toBeNull();
    expect(contentTypeForBuffer(buf({ filename: "scratch" }))).toBeNull();
    expect(contentTypeForBuffer(buf({ filename: "" }))).toBeNull();
  });
});
