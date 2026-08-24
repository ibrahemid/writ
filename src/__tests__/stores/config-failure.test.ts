import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  getConfig: vi.fn(),
  updateConfig: vi.fn(),
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

import { configStore } from "../../stores/global/config";
import { getConfig, updateConfig } from "../../services/tauri";
import { showToast } from "../../components/Notifications/Toast";
import type { WritConfig } from "../../types/config";

const mockedGetConfig = vi.mocked(getConfig);
const mockedUpdateConfig = vi.mocked(updateConfig);
const mockedToast = vi.mocked(showToast);

const STORED = {} as WritConfig;

describe("configStore failures reach the user", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    // A successful load clears whatever failure the previous test reported.
    mockedGetConfig.mockResolvedValue(STORED);
    mockedUpdateConfig.mockResolvedValue(undefined);
    await configStore.load();
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows a message when the stored settings cannot be read", async () => {
    mockedGetConfig.mockRejectedValueOnce(new Error("unreadable"));

    await configStore.load();

    expect(mockedToast).toHaveBeenCalledTimes(1);
    const [text, level] = mockedToast.mock.calls[0];
    expect(level).toBe("error");
    expect(text).toMatch(/settings/i);
  });

  it("reports a load failure once while it keeps failing", async () => {
    mockedGetConfig.mockRejectedValue(new Error("unreadable"));

    await configStore.load();
    await configStore.load();
    await configStore.load();

    expect(mockedToast).toHaveBeenCalledTimes(1);
  });

  it("shows a message when settings cannot be written", async () => {
    vi.useFakeTimers();
    mockedUpdateConfig.mockRejectedValue(new Error("read-only"));

    configStore.recordCommandUse("tab.new");
    await vi.advanceTimersByTimeAsync(1000);

    expect(mockedUpdateConfig).toHaveBeenCalled();
    expect(mockedToast).toHaveBeenCalledTimes(1);
    const [text, level] = mockedToast.mock.calls[0];
    expect(level).toBe("error");
    expect(text).toMatch(/settings/i);
  });
});
