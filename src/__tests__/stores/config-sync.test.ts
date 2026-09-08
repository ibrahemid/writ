import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  getConfig: vi.fn(),
  updateConfig: vi.fn(),
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

import { syncConfigFromDisk } from "../../stores/global/config-sync";
import { configStore } from "../../stores/global/config";
import { themeStore } from "../../stores/global/theme";
import { getConfig, updateConfig } from "../../services/tauri";
import type { AppearanceConfig, WritConfig } from "../../types/config";
import type { ThemeConfig } from "../../types/theme";

const mockedGetConfig = vi.mocked(getConfig);
const mockedUpdateConfig = vi.mocked(updateConfig);

// The fields this file asserts on; the rest are filled by the store's own
// normalization, as in config-failure.test.ts.
function storedConfig(theme: ThemeConfig, appearance: AppearanceConfig): WritConfig {
  return { theme, appearance } as unknown as WritConfig;
}

// Deliberately none of DEFAULT_CONFIG's values: a failed load falls back to
// those, so a fixture that matched them could not tell the guard from its absence.
const ON_DISK: AppearanceConfig = {
  polarity: "dark",
  accent: "terracotta",
  prose_face: "system",
  interface_text_size: null,
};

const EDITED_ON_DISK: AppearanceConfig = {
  polarity: "light",
  accent: "plum",
  prose_face: "system",
  interface_text_size: null,
};

describe("syncConfigFromDisk", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    mockedUpdateConfig.mockResolvedValue(undefined);
    mockedGetConfig.mockResolvedValue(storedConfig({ preset: "warp-dark", overrides: {} }, ON_DISK));
    await syncConfigFromDisk();
    vi.clearAllMocks();
  });

  it("moves an external appearance edit into the theme store", async () => {
    mockedGetConfig.mockResolvedValue(
      storedConfig({ preset: "writ-light", overrides: {} }, EDITED_ON_DISK),
    );

    await syncConfigFromDisk();

    expect(themeStore.appearance().accent).toBe("plum");
    expect(themeStore.appearance().polarity).toBe("light");
    expect(themeStore.presetId()).toBe("writ-light");
  });

  it("keeps the external accent when the theme is saved next", async () => {
    mockedGetConfig.mockResolvedValue(
      storedConfig({ preset: "writ-light", overrides: {} }, EDITED_ON_DISK),
    );
    await syncConfigFromDisk();

    // The payload ThemeEditor's Save builds.
    await configStore.save({
      ...configStore.config(),
      theme: themeStore.toConfig(),
      appearance: themeStore.appearance(),
    });

    const calls = mockedUpdateConfig.mock.calls;
    const written = calls[calls.length - 1][0] as WritConfig;
    expect(written.appearance.accent).toBe("plum");
    expect(written.theme.preset).toBe("writ-light");
  });

  it("leaves the theme store alone when the config cannot be read", async () => {
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mockedGetConfig.mockRejectedValue(new Error("no file"));

    await syncConfigFromDisk();

    expect(themeStore.appearance().accent).toBe("terracotta");
    expect(themeStore.presetId()).toBe("warp-dark");
    consoleSpy.mockRestore();
  });
});
