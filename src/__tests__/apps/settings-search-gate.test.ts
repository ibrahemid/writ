import { describe, it, expect, vi, beforeEach } from "vitest";

// ADR-042 section 4: settings search finds only rows that are drawn, so a row
// under an app that is off is not a result.

vi.mock("../../services/tauri", () => ({ updateConfig: vi.fn().mockResolvedValue(undefined) }));

import { configStore } from "../../stores/global/config";
import { isSettingAvailable } from "../../settings/availability";
import { SETTINGS_INDEX } from "../../settings";

beforeEach(async () => {
  for (const app of ["chat", "rewrite", "programs"] as const) await configStore.setAppOn(app, false);
});

describe("rows under an app", () => {
  it("are not available while their apps are off", () => {
    for (const id of ["ai.provider", "ai.api_key", "ai.model", "ai.chat.model", "mcp.clients"]) {
      expect(isSettingAvailable(id)).toBe(false);
    }
  });

  it("come back with either app that shares them", async () => {
    await configStore.setAppOn("rewrite", true);
    expect(isSettingAvailable("ai.api_key")).toBe(true);
    expect(isSettingAvailable("ai.chat.model")).toBe(false);
    await configStore.setAppOn("chat", true);
    expect(isSettingAvailable("ai.chat.model")).toBe(true);
    expect(isSettingAvailable("mcp.clients")).toBe(false);
    await configStore.setAppOn("programs", true);
    expect(isSettingAvailable("mcp.clients")).toBe(true);
  });

  it("leave every app's own switch findable while it is off", () => {
    for (const id of ["ai.chat.enabled", "ai.rewrite.enabled", "mcp.enabled", "apps.connections", "apps.graph", "apps.tags"]) {
      expect(SETTINGS_INDEX.find((entry) => entry.id === id)?.requires).toBeUndefined();
      expect(isSettingAvailable(id)).toBe(true);
    }
  });
});
