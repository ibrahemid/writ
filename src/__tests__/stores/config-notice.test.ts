import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  getConfig: vi.fn(),
  updateConfig: vi.fn(),
  getConfigDiagnostics: vi.fn(),
}));

vi.mock("../../stores/global/notifications", () => ({
  showToast: vi.fn(() => 7),
  dismissToast: vi.fn(),
}));

import { configStore, unreadValuesNotice } from "../../stores/global/config";
import { getConfigDiagnostics } from "../../services/tauri";
import { dismissToast, showToast } from "../../stores/global/notifications";

const mockedDiagnostics = vi.mocked(getConfigDiagnostics);

// RC-07: a launch on a config.toml with a value Writ cannot read says which
// value and where, and that settings are not saved until the file is fixed.
describe("the notice for values config.toml holds that Writ could not read", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("names the key and the line", () => {
    const text = unreadValuesNotice([
      { key: "editor.font_size", line: 2, message: "invalid type" },
    ]);
    expect(text).toContain("editor.font_size");
    expect(text).toContain("line 2");
    expect(text).toContain("config.toml");
    expect(text).toMatch(/won't be saved/);
  });

  it("counts the values after the first", () => {
    const text = unreadValuesNotice([
      { key: "editor.font_size", line: 2, message: "" },
      { key: "files.default_extension", line: 5, message: "" },
      { key: "appearance.polarity", line: 8, message: "" },
    ]);
    expect(text).toContain("editor.font_size on line 2");
    expect(text).toContain("2 other values");
  });

  it("names the line of a file that is not TOML at all", () => {
    const text = unreadValuesNotice([{ key: "", line: 3, message: "expected `]`" }]);
    expect(text).toContain("line 3");
    expect(text).toMatch(/default settings/);
  });

  it("says nothing when every value was read", () => {
    expect(unreadValuesNotice([])).toBeNull();
  });

  it("stays up until a reload reads the file in full, then goes", async () => {
    mockedDiagnostics.mockResolvedValueOnce([{ key: "editor.font_size", line: 2, message: "" }]);
    await configStore.noticeUnreadValues();
    expect(vi.mocked(showToast)).toHaveBeenCalledTimes(1);
    const [, level, duration] = vi.mocked(showToast).mock.calls[0];
    expect(level).toBe("warning");
    expect(duration).toBe(0);

    mockedDiagnostics.mockResolvedValueOnce([{ key: "editor.font_size", line: 2, message: "" }]);
    await configStore.noticeUnreadValues();
    expect(vi.mocked(showToast)).toHaveBeenCalledTimes(1);

    mockedDiagnostics.mockResolvedValueOnce([]);
    await configStore.noticeUnreadValues();
    expect(vi.mocked(dismissToast)).toHaveBeenCalledWith(7);
  });
});
