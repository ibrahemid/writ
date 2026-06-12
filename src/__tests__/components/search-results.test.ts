import { describe, it, expect } from "vitest";
import { matchedBuffers } from "../../components/Sidebar/search-results";
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

describe("matchedBuffers", () => {
  it("returns nothing for an empty query", () => {
    const active = [buf({ title: "alpha" })];
    expect(matchedBuffers("", [], active, [])).toEqual({ active: [], history: [] });
    expect(matchedBuffers("   ", [], active, [])).toEqual({ active: [], history: [] });
  });

  it("matches an OPEN buffer by title (the Bug #1 regression)", () => {
    const open = buf({ id: "open-1", title: "notes-draft", status: "active" });
    const result = matchedBuffers("draft", [], [open], []);
    expect(result.active.map((b) => b.id)).toEqual(["open-1"]);
    expect(result.history).toEqual([]);
  });

  it("matches a history buffer by title", () => {
    const old = buf({ id: "h-1", title: "old-spec", status: "history" });
    const result = matchedBuffers("spec", [], [], [old]);
    expect(result.history.map((b) => b.id)).toEqual(["h-1"]);
  });

  it("matches by FTS id across both active and history", () => {
    const open = buf({ id: "o", title: "zzz", status: "active" });
    const old = buf({ id: "h", title: "qqq", status: "history" });
    const result = matchedBuffers("content-term", ["o", "h"], [open], [old]);
    expect(result.active.map((b) => b.id)).toEqual(["o"]);
    expect(result.history.map((b) => b.id)).toEqual(["h"]);
  });

  it("returns active matches even when no history buffers exist", () => {
    const open = buf({ id: "only", title: "report", status: "active" });
    const result = matchedBuffers("report", [], [open], []);
    expect(result.active).toHaveLength(1);
    expect(result.history).toHaveLength(0);
  });
});
