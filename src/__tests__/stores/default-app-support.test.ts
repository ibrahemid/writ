import { describe, it, expect, vi, beforeEach } from "vitest";

const h = vi.hoisted(() => ({
  fetchDefaultAppTypes: vi.fn(),
  fetchDefaultAppStatus: vi.fn(),
}));

vi.mock("../../stores/global/default-app", () => ({
  fetchDefaultAppTypes: h.fetchDefaultAppTypes,
  fetchDefaultAppStatus: h.fetchDefaultAppStatus,
}));

import {
  probeDefaultAppSupport,
  hasSupportedDefaultAppTypes,
  markDefaultAppTypeSupported,
  clearDefaultAppSupport,
} from "../../stores/global/default-app-support";

function type(id: string) {
  return { id, label: id, exts: [id], utis: [`public.${id}`] };
}

describe("default-app support registry", () => {
  beforeEach(() => {
    clearDefaultAppSupport();
    h.fetchDefaultAppTypes.mockReset();
    h.fetchDefaultAppStatus.mockReset();
  });

  it("marks only non-unsupported types as supported after a probe", async () => {
    h.fetchDefaultAppTypes.mockResolvedValue([type("markdown"), type("source-code")]);
    h.fetchDefaultAppStatus.mockImplementation((id: string) =>
      Promise.resolve(id === "markdown" ? { status: "is_default" } : { status: "unsupported" }),
    );

    await probeDefaultAppSupport();
    expect(hasSupportedDefaultAppTypes()).toBe(true);

    // Dropping the one type that reported supported empties the registry, which
    // is only true if the unsupported one was never recorded.
    markDefaultAppTypeSupported("markdown", false);
    expect(hasSupportedDefaultAppTypes()).toBe(false);
  });

  it("leaves the registry empty when listing types fails", async () => {
    h.fetchDefaultAppTypes.mockRejectedValue(new Error("no IPC"));
    await probeDefaultAppSupport();
    expect(hasSupportedDefaultAppTypes()).toBe(false);
  });

  it("treats a per-type status error as unsupported", async () => {
    h.fetchDefaultAppTypes.mockResolvedValue([type("markdown")]);
    h.fetchDefaultAppStatus.mockRejectedValue(new Error("boom"));
    await probeDefaultAppSupport();
    expect(hasSupportedDefaultAppTypes()).toBe(false);
  });

  it("updates support imperatively", () => {
    markDefaultAppTypeSupported("config-data", true);
    expect(hasSupportedDefaultAppTypes()).toBe(true);
    markDefaultAppTypeSupported("config-data", false);
    expect(hasSupportedDefaultAppTypes()).toBe(false);
  });
});
