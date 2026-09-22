import { describe, it, expect, vi, afterEach } from "vitest";
import { createLayoutStore } from "../../stores/window/layout-store";
import { configStore } from "../../stores/global/config";
import type { MarkdownLayout } from "../../types/config";

// A buffer the store has not resolved yet still has to answer, and the answer
// is what the editor mounts with. For markdown that is the configured default,
// not source: resolving it a round trip later is a flash of raw markup.

vi.mock("../../services/tauri", () => ({
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
}));

function markdownDefault(setting: MarkdownLayout) {
  const held = configStore.config();
  vi.spyOn(configStore, "config").mockReturnValue({
    ...held,
    preview: { ...held.preview, default_layout_markdown: setting },
  });
}

describe("layout store: a buffer with no resolved layout", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("reads a markdown buffer as inline when that is the configured default", () => {
    markdownDefault("inline");
    const store = createLayoutStore({ windowId: 7001 });

    expect(store.get("L1", "markdown")).toEqual({ kind: "inline" });
  });

  it("reads a markdown buffer as source when that is the configured default", () => {
    markdownDefault("source");
    const store = createLayoutStore({ windowId: 7002 });

    expect(store.get("L1", "markdown")).toEqual({ kind: "source" });
  });

  it("reads every other content type as source", () => {
    markdownDefault("inline");
    const store = createLayoutStore({ windowId: 7003 });

    expect(store.get("L1", "html")).toEqual({ kind: "source" });
    expect(store.get("L2", null)).toEqual({ kind: "source" });
  });

  it("takes a resolved layout over the default", () => {
    markdownDefault("inline");
    const store = createLayoutStore({ windowId: 7004 });
    store.setLocal("L1", { kind: "source" });

    expect(store.get("L1", "markdown")).toEqual({ kind: "source" });
  });
});
