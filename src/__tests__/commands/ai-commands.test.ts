import { describe, it, expect, afterEach } from "vitest";
import { registerAiCommands, unregisterAiCommands } from "../../commands/ai";
import { getAllCommands } from "../../commands/registry";

const AI_IDS = ["ai.custom", "ai.polish", "ai.proofread", "ai.rephrase"];

describe("rewrite command registration", () => {
  afterEach(() => unregisterAiCommands());

  it("registers all four rewrite commands", () => {
    registerAiCommands();
    const ids = getAllCommands()
      .filter((c) => c.id.startsWith("ai."))
      .map((c) => c.id)
      .sort();
    expect(ids).toEqual(AI_IDS);
  });

  it("registers them app-scoped so the command palette lists them", () => {
    // The palette only shows commands with scope === "app" (see CommandPalette).
    // Editor-scoped commands would register but never appear — the smoke bug.
    registerAiCommands();
    const paletteVisible = getAllCommands().filter((c) => c.scope === "app");
    for (const id of AI_IDS) {
      expect(paletteVisible.some((c) => c.id === id)).toBe(true);
    }
  });

  it("unregisters live so disabling removes them from the palette", () => {
    registerAiCommands();
    unregisterAiCommands();
    const remaining = getAllCommands().filter((c) => c.id.startsWith("ai."));
    expect(remaining).toEqual([]);
  });
});
