import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const mocks = vi.hoisted(() => ({
  detectPlatform: vi.fn(() => "mac" as "mac" | "win" | "linux"),
  hasSupportedDefaultAppTypes: vi.fn(() => true),
}));

vi.mock("../../lib/platform", () => ({
  detectPlatform: mocks.detectPlatform,
  IS_MAC: true,
  SHOW_IN_FILE_MANAGER: "Show in Finder",
}));

vi.mock("../../stores/global/default-app-support", () => ({
  hasSupportedDefaultAppTypes: mocks.hasSupportedDefaultAppTypes,
}));

import { isSectionAvailable, isSettingAvailable } from "../../settings/availability";

describe("setting availability", () => {
  beforeEach(() => {
    mocks.detectPlatform.mockReturnValue("mac");
    mocks.hasSupportedDefaultAppTypes.mockReturnValue(true);
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
    expect(isSettingAvailable("files.inbox_folder")).toBe(true);
    expect(isSettingAvailable("editor.font_size")).toBe(true);
  });

  it("hides the file-types row when no claimable type is supported", () => {
    mocks.hasSupportedDefaultAppTypes.mockReturnValue(false);
    expect(isSettingAvailable("files.default_app")).toBe(false);
    mocks.hasSupportedDefaultAppTypes.mockReturnValue(true);
    expect(isSettingAvailable("files.default_app")).toBe(true);
  });

  it("does not ask the platform about a row no platform gates", () => {
    expect(isSettingAvailable("editor.tab_size")).toBe(true);
    expect(mocks.detectPlatform).not.toHaveBeenCalled();
  });

  it("hides the file-types row off macOS, where nothing can be claimed", () => {
    for (const platform of ["win", "linux"] as const) {
      mocks.detectPlatform.mockReturnValue(platform);
      expect(isSettingAvailable("files.default_app"), platform).toBe(false);
    }
  });

  // Files holds the file-types row and nothing else, so off macOS the section
  // would open on its heading alone.
  it("drops the Files section off macOS and keeps it on macOS", () => {
    mocks.detectPlatform.mockReturnValue("win");
    expect(isSectionAvailable("files")).toBe(false);
    mocks.detectPlatform.mockReturnValue("mac");
    expect(isSectionAvailable("files")).toBe(true);
  });

  it("keeps every other section on every platform", () => {
    for (const platform of ["mac", "win", "linux"] as const) {
      mocks.detectPlatform.mockReturnValue(platform);
      for (const section of [
        "notes",
        "editor",
        "preview",
        "ai",
        "appearance",
        "updates",
        "shortcuts",
        "advanced",
      ] as const) {
        expect(isSectionAvailable(section), `${platform}/${section}`).toBe(true);
      }
    }
  });

  // NAV_ITEMS is built once at import, so no mount test can watch it filter.
  it("the nav rail is built from the sections this platform can fill", () => {
    const source = readFileSync(
      resolve(process.cwd(), "src/components/SettingsModal/SettingsModal.tsx"),
      "utf8",
    );
    expect(source).toMatch(/const NAV_ITEMS[^=]*=\s*SECTION_ORDER\.filter\(\s*isSectionAvailable/);
    expect(source).toMatch(/isSectionAvailable\(section\) \? section : DEFAULT_SECTION/);
  });

  // The section gate answers before any type has reported in; asking the
  // registry would hide Files on macOS too, and it would never mount to answer.
  it("offers the Files section before any type has reported support", () => {
    mocks.hasSupportedDefaultAppTypes.mockReturnValue(false);
    expect(isSectionAvailable("files")).toBe(true);
  });
});
