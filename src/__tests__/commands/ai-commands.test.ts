import { describe, it, expect, afterEach } from "vitest";
import { rewriteCommands } from "../../commands/ai";
import { registerCommand as registerRewrite, unregisterCommand as unregisterRewrite } from "../../commands/registry";
import { getAllCommands } from "../../commands/registry";
import { REWRITE_ACTIONS, REWRITE_COMMAND_IDS } from "../../commands/rewrite-actions";

// Derived from the one action table, so a new action cannot be added without
// this file covering it.
const AI_IDS = [...REWRITE_COMMAND_IDS].sort();

describe("rewrite command registration", () => {
  afterEach(() => rewriteCommands().forEach((c) => unregisterRewrite(c.id)));

  it("registers every rewrite command in the table", () => {
    rewriteCommands().forEach(registerRewrite);
    const ids = getAllCommands()
      .filter((c) => c.id.startsWith("ai."))
      .map((c) => c.id)
      .sort();
    expect(ids).toEqual(AI_IDS);
    expect(ids).toHaveLength(5);
  });

  it("offers proofread, rephrase, polish, improve prompt and custom", () => {
    // The reported symptom was a palette that surfaced only the custom rewrite.
    expect(REWRITE_ACTIONS.map((a) => a.id)).toEqual([
      "proofread",
      "rephrase",
      "polish",
      "improve_prompt",
      "custom",
    ]);
  });

  it("registers them app-scoped so the command palette lists them", () => {
    // The palette only shows commands with scope === "app" (see CommandPalette).
    // Editor-scoped commands would register but never appear — the smoke bug.
    rewriteCommands().forEach(registerRewrite);
    const paletteVisible = getAllCommands().filter((c) => c.scope === "app");
    for (const id of AI_IDS) {
      expect(paletteVisible.some((c) => c.id === id)).toBe(true);
    }
  });

  it("unregisters live so disabling removes them from the palette", () => {
    rewriteCommands().forEach(registerRewrite);
    rewriteCommands().forEach((c) => unregisterRewrite(c.id));
    const remaining = getAllCommands().filter((c) => c.id.startsWith("ai."));
    expect(remaining).toEqual([]);
  });
});
