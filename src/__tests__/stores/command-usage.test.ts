import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  getCommandUsage: vi.fn(),
  recordCommandUses: vi.fn().mockResolvedValue(undefined),
  clearCommandUsage: vi.fn().mockResolvedValue(undefined),
  pruneCommandUsage: vi.fn().mockResolvedValue(undefined),
}));

import { commandUsageStore } from "../../stores/global/command-usage";
import {
  clearCommandUsage,
  getCommandUsage,
  pruneCommandUsage,
  recordCommandUses,
} from "../../services/tauri";

const mockedGet = vi.mocked(getCommandUsage);
const mockedRecord = vi.mocked(recordCommandUses);

// Command usage lives in writ.db (ADR-048); the store keeps the palette's copy
// and writes the runs a burst added once the burst ends.
describe("commandUsageStore", () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    await commandUsageStore.clear();
    vi.clearAllMocks();
  });
  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("counts a run and stamps when it happened", () => {
    commandUsageStore.record("palette.open", 1_715_000_000_000);
    commandUsageStore.record("palette.open", 1_715_000_001_000);

    expect(commandUsageStore.usage()["palette.open"]).toEqual({
      count: 2,
      last_used_ms: 1_715_000_001_000,
    });
  });

  it("writes a burst of runs once, added up per command", async () => {
    commandUsageStore.record("editor.duplicateLine", 10);
    commandUsageStore.record("editor.duplicateLine", 20);
    commandUsageStore.record("tab.new", 15);

    await vi.advanceTimersByTimeAsync(749);
    expect(mockedRecord).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1);
    expect(mockedRecord).toHaveBeenCalledTimes(1);
    expect(mockedRecord.mock.calls[0][0]).toEqual([
      { id: "editor.duplicateLine", count: 2, last_used_ms: 20 },
      { id: "tab.new", count: 1, last_used_ms: 15 },
    ]);
  });

  it("loads what writ.db holds and keeps runs recorded during the read", async () => {
    let answer: (value: Record<string, { count: number; last_used_ms: number }>) => void = () => {};
    mockedGet.mockReturnValueOnce(new Promise((resolve) => (answer = resolve)));

    const loading = commandUsageStore.load();
    commandUsageStore.record("tab.new", 50);
    answer({ "tab.new": { count: 4, last_used_ms: 30 }, "palette.open": { count: 1, last_used_ms: 5 } });
    await loading;

    expect(commandUsageStore.usage()["tab.new"]).toEqual({ count: 5, last_used_ms: 50 });
    expect(commandUsageStore.usage()["palette.open"]).toEqual({ count: 1, last_used_ms: 5 });
  });

  it("clears every command and drops runs not yet written", async () => {
    commandUsageStore.record("x.y", 100);

    await commandUsageStore.clear();
    await vi.advanceTimersByTimeAsync(1000);

    expect(commandUsageStore.usage()).toEqual({});
    expect(vi.mocked(clearCommandUsage)).toHaveBeenCalledTimes(1);
    expect(mockedRecord).not.toHaveBeenCalled();
  });

  it("forgets commands that no longer exist", () => {
    commandUsageStore.record("live.cmd", 1);
    commandUsageStore.record("removed.cmd", 2);

    commandUsageStore.prune(new Set(["live.cmd"]));

    expect(Object.keys(commandUsageStore.usage())).toEqual(["live.cmd"]);
    expect(vi.mocked(pruneCommandUsage)).toHaveBeenCalledWith(["live.cmd"]);
  });
});
