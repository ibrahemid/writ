import { describe, it, expect } from "vitest";
import { runtimeRunning } from "../../lib/local-runtimes";

describe("runtimeRunning", () => {
  it("reads a runtime's probe answer by its provider id", () => {
    const probe = { ollama: false, lmstudio: true, llamacpp: true, jan: false };
    expect(runtimeRunning(probe, "llamacpp")).toBe(true);
    expect(runtimeRunning(probe, "jan")).toBe(false);
    expect(runtimeRunning(probe, "lmstudio")).toBe(true);
    expect(runtimeRunning(probe, "groq")).toBeNull();
    expect(runtimeRunning(null, "ollama")).toBeNull();
  });
});
