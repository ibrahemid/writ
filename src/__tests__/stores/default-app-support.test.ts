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
  isDefaultAppTypeSupported,
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

    expect(isDefaultAppTypeSupported("markdown")).toBe(true);
    expect(isDefaultAppTypeSupported("source-code")).toBe(false);
  });

  it("leaves the registry empty when listing types fails", async () => {
    h.fetchDefaultAppTypes.mockRejectedValue(new Error("no IPC"));
    await probeDefaultAppSupport();
    expect(isDefaultAppTypeSupported("markdown")).toBe(false);
  });

  it("treats a per-type status error as unsupported", async () => {
    h.fetchDefaultAppTypes.mockResolvedValue([type("markdown")]);
    h.fetchDefaultAppStatus.mockRejectedValue(new Error("boom"));
    await probeDefaultAppSupport();
    expect(isDefaultAppTypeSupported("markdown")).toBe(false);
  });

  it("updates support imperatively", () => {
    markDefaultAppTypeSupported("config-data", true);
    expect(isDefaultAppTypeSupported("config-data")).toBe(true);
    markDefaultAppTypeSupported("config-data", false);
    expect(isDefaultAppTypeSupported("config-data")).toBe(false);
  });
});
