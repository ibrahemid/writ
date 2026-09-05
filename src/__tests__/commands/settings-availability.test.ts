import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const mocks = vi.hoisted(() => ({
  detectPlatform: vi.fn(() => "mac" as "mac" | "win" | "linux"),
  isDefaultAppTypeSupported: vi.fn(() => true),
}));

vi.mock("../../lib/platform", () => ({
  detectPlatform: mocks.detectPlatform,
  IS_MAC: true,
  SHOW_IN_FILE_MANAGER: "Show in Finder",
}));

vi.mock("../../stores/global/default-app-support", () => ({
  isDefaultAppTypeSupported: mocks.isDefaultAppTypeSupported,
}));

import { isSettingAvailable } from "../../settings/availability";

describe("setting availability", () => {
  beforeEach(() => {
    mocks.detectPlatform.mockReturnValue("mac");
    mocks.isDefaultAppTypeSupported.mockReturnValue(true);
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("offers the command-line row where the app can put the command", () => {
    for (const platform of ["mac", "linux"] as const) {
      mocks.detectPlatform.mockReturnValue(platform);
      expect(isSettingAvailable("files.cli")).toBe(true);
    }
  });

  it("hides the command-line row on Windows, where the installer puts it on the PATH", () => {
    mocks.detectPlatform.mockReturnValue("win");
    expect(isSettingAvailable("files.cli")).toBe(false);
  });

  it("leaves every other row alone on Windows", () => {
    mocks.detectPlatform.mockReturnValue("win");
    expect(isSettingAvailable("files.autosave")).toBe(true);
    expect(isSettingAvailable("editor.font_size")).toBe(true);
  });

  it("still reads the default-app store for a default-app row", () => {
    mocks.isDefaultAppTypeSupported.mockReturnValue(false);
    expect(isSettingAvailable("files.default_app.markdown")).toBe(false);
    expect(mocks.isDefaultAppTypeSupported).toHaveBeenCalledWith("markdown");
  });

  it("does not ask the platform about a row no platform gates", () => {
    expect(isSettingAvailable("editor.tab_size")).toBe(true);
    expect(mocks.detectPlatform).not.toHaveBeenCalled();
  });
});
