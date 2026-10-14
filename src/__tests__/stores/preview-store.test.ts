import { describe, it, expect, vi, beforeEach } from "vitest";

// A render checks the file's preview-scripts opt-in, so what it answers is what
// the status bar shows: a render that finds the opt-in lapsed turns the field
// off without a read of its own (ADR-043).

const mocks = vi.hoisted(() => ({
  render: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  previewForceRender: mocks.render,
  previewClose: vi.fn().mockResolvedValue(undefined),
  previewGetScripts: vi.fn(),
  previewSetScripts: vi.fn(),
}));

import { createPreviewStore } from "../../stores/window/preview-store";
import { previewScriptsStore } from "../../stores/global/preview-scripts";

function rendered(scriptsEnabled: boolean) {
  return {
    kind: "rendered" as const,
    used_fallback_stylesheet: false,
    parser_warnings: [],
    scripts_enabled: scriptsEnabled,
  };
}

describe("preview store render", () => {
  beforeEach(() => {
    mocks.render.mockReset();
  });

  it("hands the status bar each render's scripts verdict", async () => {
    const store = createPreviewStore();

    mocks.render.mockResolvedValue(rendered(true));
    await store.render("verdict-1", "html", "<p>x</p>", "light", 1);
    expect(previewScriptsStore.enabled("verdict-1")).toBe(true);

    mocks.render.mockResolvedValue(rendered(false));
    await store.render("verdict-1", "html", "<p>x</p>", "light", 1);
    expect(previewScriptsStore.enabled("verdict-1")).toBe(false);
  });

  it("says nothing about scripts for a render that did not happen", async () => {
    const store = createPreviewStore();
    mocks.render.mockResolvedValue({ kind: "failed", message: "document is too large" });

    await store.render("verdict-2", "html", "<p>x</p>", "light", 1);

    expect(previewScriptsStore.enabled("verdict-2")).toBeUndefined();
  });
});

describe("a render whose work stopped (ADR-047)", () => {
  beforeEach(() => {
    mocks.render.mockReset();
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  it("answers as a failed render the pane shows", async () => {
    mocks.render.mockRejectedValue(
      "ERR_COMMAND_STOPPED: the command stopped before it answered (task panicked)",
    );
    const result = await createPreviewStore().render("b1", "text/markdown", "# hi", "light", 1);
    expect(result.kind).toBe("failed");
    if (result.kind === "failed") {
      expect(result.message).not.toContain("ERR_COMMAND_STOPPED");
      expect(result.message.length).toBeGreaterThan(0);
    }
  });

  it("leaves any other rejection to the caller", async () => {
    mocks.render.mockRejectedValue("ERR_SOMETHING_ELSE");
    await expect(
      createPreviewStore().render("b1", "text/markdown", "# hi", "light", 1),
    ).rejects.toBe("ERR_SOMETHING_ELSE");
  });
});

