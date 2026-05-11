import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  listTransforms: vi.fn(),
  applyTransform: vi.fn(),
}));

import { registerTransformCommands } from "../../commands/transforms";
import { getAllCommands, getCommand } from "../../commands/registry";
import { editorStore } from "../../stores/editor";
import * as tauriApi from "../../services/tauri";

const mockedApi = vi.mocked(tauriApi);

function clearRegistry() {
  for (const cmd of getAllCommands()) {
    // Overwrite each id with a placeholder, then drop placeholders.
    cmd.execute = () => {};
  }
}

describe("registerTransformCommands", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    clearRegistry();
    editorStore.registerView(null);
  });

  afterEach(() => {
    editorStore.registerView(null);
  });

  it("registers one transform.<id> command per descriptor with the 'Transform: ' prefix", async () => {
    mockedApi.listTransforms.mockResolvedValue([
      {
        id: "trim_leading_whitespace",
        metadata: {
          label: "Trim Leading Whitespace",
          description: "Remove leading spaces and tabs from each line.",
          category: "whitespace",
        },
      },
      {
        id: "dedent",
        metadata: {
          label: "Dedent",
          description: "Remove shared leading indentation.",
          category: "indentation",
        },
      },
    ]);

    await registerTransformCommands();

    const trim = getCommand("transform.trim_leading_whitespace");
    const dedent = getCommand("transform.dedent");

    expect(trim).toBeDefined();
    expect(trim!.label).toBe("Transform: Trim Leading Whitespace");
    expect(trim!.scope).toBe("app");

    expect(dedent).toBeDefined();
    expect(dedent!.label).toBe("Transform: Dedent");
    expect(dedent!.scope).toBe("app");
  });

  it("commands invoke applyTransform with the descriptor id", async () => {
    mockedApi.listTransforms.mockResolvedValue([
      {
        id: "smart_to_straight_quotes",
        metadata: {
          label: "Smart → Straight Quotes",
          description: "Replace curly quotes.",
          category: "punctuation",
        },
      },
    ]);
    mockedApi.applyTransform.mockResolvedValue("transformed");

    const applySpy = vi
      .spyOn(editorStore, "applyEditToActiveBuffer")
      .mockResolvedValue({ applied: false, reason: "no-active-view" });

    await registerTransformCommands();

    const cmd = getCommand("transform.smart_to_straight_quotes");
    expect(cmd).toBeDefined();
    cmd!.execute();
    await Promise.resolve();

    expect(applySpy).toHaveBeenCalledTimes(1);
    const call = applySpy.mock.calls[0][0];
    expect(call.useSelectionIfPresent).toBe(true);

    await call.transform("input");
    expect(mockedApi.applyTransform).toHaveBeenCalledWith("smart_to_straight_quotes", "input");

    applySpy.mockRestore();
  });
});
