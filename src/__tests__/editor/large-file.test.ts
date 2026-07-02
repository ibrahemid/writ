import { describe, expect, it } from "vitest";
import {
  MAX_NORMAL_LINE_LENGTH,
  THRESHOLD_NORMAL_BYTES,
  editorModeForContent,
  hasLongLines,
} from "../../editor/large-file";
import type { BufferDocument } from "../../types/buffer";

function buffer(overrides: Partial<BufferDocument> = {}): BufferDocument {
  return {
    id: "b1",
    title: "file.txt",
    filename: "file.txt",
    status: "active",
    language: null,
    source_path: "/tmp/file.txt",
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "",
    updated_at: "",
    closed_at: null,
    read_only: false,
    size_bytes: 1024,
    ...overrides,
  };
}

describe("hasLongLines", () => {
  it("is false for ordinary multi-line text", () => {
    const content = Array.from({ length: 500 }, () => "a normal line of source code").join("\n");
    expect(hasLongLines(content)).toBe(false);
  });

  it("is false for an empty document", () => {
    expect(hasLongLines("")).toBe(false);
  });

  it("is true for a single line at the threshold + 1", () => {
    expect(hasLongLines("x".repeat(MAX_NORMAL_LINE_LENGTH + 1))).toBe(true);
  });

  it("is false for a single line exactly at the threshold", () => {
    expect(hasLongLines("x".repeat(MAX_NORMAL_LINE_LENGTH))).toBe(false);
  });

  it("detects one long line buried among short lines", () => {
    const content = ["short", "lines", "x".repeat(MAX_NORMAL_LINE_LENGTH + 1), "more"].join("\n");
    expect(hasLongLines(content)).toBe(true);
  });

  it("detects a long final line with no trailing newline", () => {
    const content = "short\n" + "y".repeat(MAX_NORMAL_LINE_LENGTH + 5);
    expect(hasLongLines(content)).toBe(true);
  });

  it("honours a custom limit", () => {
    expect(hasLongLines("abcdef", 5)).toBe(true);
    expect(hasLongLines("abcde", 5)).toBe(false);
  });

  it("treats CRLF and bare-CR as line breaks, not one giant line", () => {
    const short = "x".repeat(MAX_NORMAL_LINE_LENGTH - 1);
    expect(hasLongLines([short, short, short].join("\r\n"))).toBe(false);
    expect(hasLongLines([short, short, short].join("\r"))).toBe(false);
  });
});

describe("editorModeForContent", () => {
  it("returns Normal for a small, ordinary file", () => {
    expect(editorModeForContent(buffer(), "hello world\nsecond line").kind).toBe("Normal");
  });

  it("returns Binary for a read-only buffer regardless of content", () => {
    expect(editorModeForContent(buffer({ read_only: true }), "anything").kind).toBe("Binary");
  });

  it("keeps a file exactly at the byte threshold Normal", () => {
    const mode = editorModeForContent(buffer({ size_bytes: THRESHOLD_NORMAL_BYTES }), "short");
    expect(mode.kind).toBe("Normal");
  });

  it("returns LargeFile when size exceeds the byte threshold", () => {
    const mode = editorModeForContent(buffer({ size_bytes: THRESHOLD_NORMAL_BYTES + 1 }), "short");
    expect(mode.kind).toBe("LargeFile");
  });

  it("upgrades a sub-threshold long-line file to LongLines", () => {
    const minified = "x".repeat(MAX_NORMAL_LINE_LENGTH + 1);
    const mode = editorModeForContent(buffer({ size_bytes: minified.length }), minified);
    expect(mode.kind).toBe("LongLines");
  });

  it("prefers the byte tier over line shape for a large long-line file", () => {
    const mode = editorModeForContent(
      buffer({ size_bytes: THRESHOLD_NORMAL_BYTES + 1 }),
      "x".repeat(MAX_NORMAL_LINE_LENGTH + 1),
    );
    expect(mode.kind).toBe("LargeFile");
  });
});
