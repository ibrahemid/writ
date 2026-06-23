import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const fillMock = vi.fn();
const estimateMock = vi.fn();

vi.mock("../../services/tauri", () => ({
  promptFillPlaceholders: (text: string, values: Record<string, string>) =>
    fillMock(text, values),
  promptEstimateTokens: (text: string) => estimateMock(text),
}));

import { promptEstimateStore } from "../../stores/global/prompt-estimate";

describe("promptEstimateStore", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    fillMock.mockReset();
    estimateMock.mockReset();
    promptEstimateStore.reset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("fills the template then counts tokens of the result", async () => {
    fillMock.mockResolvedValue("Hello Ada");
    estimateMock.mockResolvedValue(3);

    promptEstimateStore.request("Hello {{name}}", { name: "Ada" });
    await vi.advanceTimersByTimeAsync(400);

    expect(fillMock).toHaveBeenCalledWith("Hello {{name}}", { name: "Ada" });
    expect(estimateMock).toHaveBeenCalledWith("Hello Ada");
    expect(promptEstimateStore.count()).toBe(3);
  });

  it("debounces rapid requests to a single estimate", async () => {
    fillMock.mockResolvedValue("x");
    estimateMock.mockResolvedValue(1);

    promptEstimateStore.request("t {{a}}", { a: "1" });
    promptEstimateStore.request("t {{a}}", { a: "12" });
    promptEstimateStore.request("t {{a}}", { a: "123" });
    await vi.advanceTimersByTimeAsync(400);

    expect(fillMock).toHaveBeenCalledTimes(1);
    expect(fillMock).toHaveBeenCalledWith("t {{a}}", { a: "123" });
  });

  it("clears the count for an empty template without calling the backend", () => {
    promptEstimateStore.request("   ", { a: "1" });
    expect(promptEstimateStore.count()).toBeNull();
    expect(fillMock).not.toHaveBeenCalled();
  });

  it("ignores a stale in-flight result after reset", async () => {
    let resolveFill: (v: string) => void = () => {};
    fillMock.mockReturnValue(new Promise<string>((r) => (resolveFill = r)));
    estimateMock.mockResolvedValue(9);

    promptEstimateStore.request("t {{a}}", { a: "1" });
    await vi.advanceTimersByTimeAsync(400);
    promptEstimateStore.reset();
    resolveFill("filled");
    await Promise.resolve();
    await Promise.resolve();

    expect(promptEstimateStore.count()).toBeNull();
  });
});
