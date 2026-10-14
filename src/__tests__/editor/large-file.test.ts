import { describe, expect, it } from "vitest";
import { EditorState, Text } from "@codemirror/state";
import {
  HIGHLIGHT_LINE_LIMIT,
  MAX_NORMAL_LINE_LENGTH,
  THRESHOLD_NORMAL_BYTES,
  WRAP_LINE_LIMIT,
  editorModeForContent,
  editorTierForContent,
  featuresFor,
  hasLongLines,
  longestLine,
  savesAsChanges,
  tierForDocument,
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
    line_ending: "lf",
    encoding: "UTF-8",
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

describe("editorTierForContent", () => {
  it("measures the longest line of a long-lined file up to the scan limit", () => {
    const content = ["short", "x".repeat(MAX_NORMAL_LINE_LENGTH + 10), "end"].join("\n");
    expect(editorTierForContent(buffer(), content)).toEqual({
      mode: { kind: "LongLines" },
      longest: MAX_NORMAL_LINE_LENGTH + 10,
    });
  });

  it("treats text in hand over the threshold as large whatever the row says", () => {
    const grown = "y".repeat(THRESHOLD_NORMAL_BYTES + 1);
    expect(editorTierForContent(buffer({ size_bytes: 1024 }), grown).mode.kind).toBe("LargeFile");
  });
});

describe("longestLine", () => {
  it("stops counting once past the limit it was given", () => {
    expect(longestLine("x".repeat(500), 100)).toBe(101);
    expect(longestLine("ab\nabcd\r\nabc")).toBe(4);
  });
});

describe("featuresFor", () => {
  it("gives a normal file everything", () => {
    expect(Object.values(featuresFor({ kind: "Normal" }, 0)).every(Boolean)).toBe(true);
  });

  it("keeps a long-lined file's rendering, links and direction and turns spelling off", () => {
    const features = featuresFor({ kind: "LongLines" }, MAX_NORMAL_LINE_LENGTH + 1);
    expect(features).toMatchObject({
      wrap: true,
      highlight: true,
      markdown: true,
      links: true,
      direction: true,
      spelling: false,
      search: true,
    });
  });

  it("turns wrapping and highlighting off past their line limits", () => {
    expect(featuresFor({ kind: "LongLines" }, WRAP_LINE_LIMIT + 1).wrap).toBe(false);
    expect(featuresFor({ kind: "LongLines" }, HIGHLIGHT_LINE_LIMIT + 1).highlight).toBe(false);
  });

  it("lets the person turn wrapping and highlighting on for a large file", () => {
    expect(featuresFor({ kind: "LargeFile" }, 0)).toMatchObject({ wrap: false, highlight: false });
    expect(featuresFor({ kind: "LargeFile" }, 0, { wrap: true, highlight: true })).toMatchObject({
      wrap: true,
      highlight: true,
      markdown: false,
      search: false,
    });
    expect(featuresFor({ kind: "LongLines" }, WRAP_LINE_LIMIT + 1, { wrap: true }).wrap).toBe(true);
    expect(featuresFor({ kind: "LongLines" }, 0, { highlight: false }).highlight).toBe(false);
  });

  it("gives a binary file nothing to turn on", () => {
    expect(Object.values(featuresFor({ kind: "Binary" }, 0, { wrap: true })).some(Boolean)).toBe(false);
  });
});

describe("tierForDocument", () => {
  it("calls for the large tier after a paste takes a small note across 5 MiB", () => {
    const small = EditorState.create({ doc: "a note\n" });
    const pasted = small.update({ changes: { from: 7, insert: "line of pasted text\n".repeat(280_000) } }).state;
    expect(tierForDocument(small.doc).mode.kind).toBe("Normal");
    expect(pasted.doc.length).toBeGreaterThan(THRESHOLD_NORMAL_BYTES);
    expect(tierForDocument(pasted.doc).mode.kind).toBe("LargeFile");
  });

  it("counts UTF-8 bytes, so text under 5 Mi code units can still be large", () => {
    const cjk = Text.of(["中".repeat(2_000_000)]);
    expect(cjk.length).toBeLessThan(THRESHOLD_NORMAL_BYTES);
    expect(tierForDocument(cjk).mode.kind).toBe("LargeFile");
  });

  it("calls for the normal tier once a large document is cut back", () => {
    expect(tierForDocument(Text.of(["short", "lines"])).mode.kind).toBe("Normal");
    expect(tierForDocument(Text.of(["x".repeat(MAX_NORMAL_LINE_LENGTH + 1)])).mode.kind).toBe("LongLines");
  });
});

describe("savesAsChanges", () => {
  it("sends changes from the large tiers and LongLines, and the whole text otherwise", () => {
    expect(savesAsChanges({ kind: "LargeFile" })).toBe(true);
    expect(savesAsChanges({ kind: "LargeFileConfirm" })).toBe(true);
    expect(savesAsChanges({ kind: "LongLines" })).toBe(true);
    expect(savesAsChanges({ kind: "Normal" })).toBe(false);
    expect(savesAsChanges(null)).toBe(false);
  });
});
