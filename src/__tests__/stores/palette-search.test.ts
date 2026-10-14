import { describe, it, expect, vi } from "vitest";

const order: string[] = [];

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn(async () => {
    order.push("flush");
    return { ok: true, failures: [] };
  }),
}));
vi.mock("../../services/tauri", () => ({
  searchBuffers: vi.fn(async (query: string) => {
    order.push(`search ${query}`);
    return { hits: [], total: 0 };
  }),
  searchNotesByName: vi.fn(async () => []),
}));

import { paletteSearch } from "../../stores/global/palette-search";
import { createContentProvider } from "../../commands/providers/content-provider";
import { flushAutosave } from "../../services/autosave";

describe("the one text search entry point (F25)", () => {
  it("writes pending saves before it asks the index, so text typed a moment ago is found", async () => {
    order.length = 0;
    await paletteSearch.text("marzipan");
    expect(order).toEqual(["flush", "search marzipan"]);
  });

  it("is the entry the palette's Text section searches through", async () => {
    order.length = 0;
    await createContentProvider().query("marzipan", new AbortController().signal, "content");
    expect(order).toEqual(["flush", "search marzipan"]);
  });

  it("still searches when the flush fails", async () => {
    order.length = 0;
    vi.mocked(flushAutosave).mockRejectedValueOnce(new Error("disk full"));
    await paletteSearch.text("marzipan");
    expect(order).toEqual(["search marzipan"]);
  });
});
