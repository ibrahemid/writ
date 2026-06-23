import { describe, it, expect } from "vitest";
import { buildSearchRows, highlightLiteral } from "../../components/Sidebar/search-results";
import type { SearchHit } from "../../services/tauri";
import type { BufferDocument } from "../../types/buffer";

let n = 0;
function buf(overrides: Partial<BufferDocument> = {}): BufferDocument {
  n++;
  return {
    id: overrides.id ?? `b-${n}`,
    title: overrides.title ?? `Buffer ${n}`,
    filename: overrides.filename ?? `b-${n}.md`,
    status: overrides.status ?? "active",
    language: overrides.language ?? null,
    source_path: overrides.source_path ?? null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: n,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: overrides.closed_at ?? null,
    read_only: overrides.read_only ?? false,
    size_bytes: overrides.size_bytes ?? 0,
  };
}

function hit(overrides: Partial<SearchHit> = {}): SearchHit {
  return {
    buffer_id: overrides.buffer_id ?? "h",
    title: overrides.title ?? "hit.md",
    line: overrides.line !== undefined ? overrides.line : 1,
    snippet: overrides.snippet ?? [{ text: "snippet", matched: false }],
  };
}

describe("buildSearchRows", () => {
  it("maps backend hits to rows and tags their source", () => {
    const open = buf({ id: "o", title: "open.md", status: "active" });
    const old = buf({ id: "h", title: "old.md", status: "history" });
    const rows = buildSearchRows(
      [hit({ buffer_id: "o", title: "open.md", line: 3 }), hit({ buffer_id: "h", title: "old.md", line: null })],
      "term",
      [open],
      [old],
    );
    expect(rows.map((r) => [r.id, r.source, r.line])).toEqual([
      ["o", "active", 3],
      ["h", "history", null],
    ]);
  });

  it("keeps a large/binary buffer findable by title even though it is never indexed", () => {
    // The classic regression: indexing is gated on size, so a big buffer is
    // absent from FTS hits and must still surface via the title pass.
    const huge = buf({ id: "big", title: "huge-log.txt", status: "active", size_bytes: 99_000_000 });
    const rows = buildSearchRows([], "huge", [huge], []);
    expect(rows).toHaveLength(1);
    expect(rows[0].id).toBe("big");
    expect(rows[0].line).toBeNull();
    expect(rows[0].segments.some((s) => s.matched)).toBe(true);
  });

  it("does not duplicate a buffer already present as a backend hit", () => {
    const open = buf({ id: "dup", title: "dup-report.md", status: "active" });
    const rows = buildSearchRows(
      [hit({ buffer_id: "dup", title: "dup-report.md", line: 5 })],
      "report",
      [open],
      [],
    );
    expect(rows).toHaveLength(1);
    expect(rows[0].line).toBe(5);
  });

  it("returns only backend hits for an empty query", () => {
    const rows = buildSearchRows([hit({ buffer_id: "x" })], "", [buf()], [buf()]);
    expect(rows.map((r) => r.id)).toEqual(["x"]);
  });
});

describe("highlightLiteral", () => {
  it("splits text around case-insensitive query occurrences", () => {
    expect(highlightLiteral("Report Draft report", "report")).toEqual([
      { text: "Report", matched: true },
      { text: " Draft ", matched: false },
      { text: "report", matched: true },
    ]);
  });

  it("returns the whole text unmatched for an empty query", () => {
    expect(highlightLiteral("anything", "  ")).toEqual([{ text: "anything", matched: false }]);
  });

  it("returns the whole text unmatched when there is no occurrence", () => {
    expect(highlightLiteral("nothing here", "zzz")).toEqual([
      { text: "nothing here", matched: false },
    ]);
  });
});
