import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  promptEstimateTokens: vi.fn(),
}));

import { tokenEstimateStore, formatTokenCount } from "../../stores/global/token-estimate";
import * as tauriApi from "../../services/tauri";

const mockedApi = vi.mocked(tauriApi);

describe("formatTokenCount", () => {
  it("renders small counts verbatim", () => {
    expect(formatTokenCount(0)).toBe("0");
    expect(formatTokenCount(7)).toBe("7");
    expect(formatTokenCount(999)).toBe("999");
  });

  it("renders thousands compactly with one decimal", () => {
    expect(formatTokenCount(1000)).toBe("1k");
    expect(formatTokenCount(1234)).toBe("1.2k");
    expect(formatTokenCount(9950)).toBe("10k");
  });

  it("drops the decimal at ten thousand and above", () => {
    expect(formatTokenCount(10000)).toBe("10k");
    expect(formatTokenCount(123456)).toBe("123k");
  });
});

describe("tokenEstimateStore", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    tokenEstimateStore.reset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("clears the count immediately for empty text without calling IPC", () => {
    tokenEstimateStore.request("");
    expect(tokenEstimateStore.count()).toBeNull();
    vi.advanceTimersByTime(1000);
    expect(mockedApi.promptEstimateTokens).not.toHaveBeenCalled();
  });

  it("treats whitespace-only text as empty", () => {
    tokenEstimateStore.request("   \n\t ");
    vi.advanceTimersByTime(1000);
    expect(tokenEstimateStore.count()).toBeNull();
    expect(mockedApi.promptEstimateTokens).not.toHaveBeenCalled();
  });

  it("debounces the estimate by 500ms", async () => {
    mockedApi.promptEstimateTokens.mockResolvedValue(42);

    tokenEstimateStore.request("some text");
    vi.advanceTimersByTime(499);
    expect(mockedApi.promptEstimateTokens).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1);
    await vi.runAllTimersAsync();
    expect(mockedApi.promptEstimateTokens).toHaveBeenCalledWith("some text");
    expect(tokenEstimateStore.count()).toBe(42);
  });

  it("collapses rapid edits into one IPC call for the latest text", async () => {
    mockedApi.promptEstimateTokens.mockResolvedValue(7);

    tokenEstimateStore.request("a");
    vi.advanceTimersByTime(200);
    tokenEstimateStore.request("ab");
    vi.advanceTimersByTime(200);
    tokenEstimateStore.request("abc");
    await vi.runAllTimersAsync();

    expect(mockedApi.promptEstimateTokens).toHaveBeenCalledTimes(1);
    expect(mockedApi.promptEstimateTokens).toHaveBeenCalledWith("abc");
  });

  it("ignores a stale response that resolves after a newer request", async () => {
    mockedApi.promptEstimateTokens.mockResolvedValue(10);
    tokenEstimateStore.request("first");
    await vi.runAllTimersAsync();
    expect(tokenEstimateStore.count()).toBe(10);

    tokenEstimateStore.request("");
    expect(tokenEstimateStore.count()).toBeNull();
    await vi.runAllTimersAsync();
    expect(tokenEstimateStore.count()).toBeNull();
  });

  it("clears the count when the IPC call fails", async () => {
    mockedApi.promptEstimateTokens.mockRejectedValue(new Error("ipc down"));
    tokenEstimateStore.request("text");
    await vi.runAllTimersAsync();
    expect(tokenEstimateStore.count()).toBeNull();
  });
});
