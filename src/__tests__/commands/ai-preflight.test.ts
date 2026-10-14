import { describe, it, expect, vi, beforeEach } from "vitest";

const mocks = vi.hoisted(() => ({
  requestConfirm: vi.fn(),
  showToast: vi.fn(),
  openSettings: vi.fn(),
  endpointState: vi.fn(),
  consentHost: vi.fn(),
  start: vi.fn(),
  config: vi.fn(),
  connectionStatus: vi.fn(() => null),
  getSelectionRange: vi.fn(),
  currentBufferId: vi.fn(() => "buf-1"),
}));

vi.mock("../../stores/global/dialogs", () => ({
  requestConfirm: mocks.requestConfirm,
}));

vi.mock("../../stores/global/notifications", () => ({
  showToast: mocks.showToast,
}));

vi.mock("../../components/SettingsModal/SettingsModal", () => ({
  default: () => null,
  openSettings: mocks.openSettings,
}));

vi.mock("../../stores/global/ai-rewrite", () => ({
  aiRewriteStore: {
    endpointState: mocks.endpointState,
    consentHost: mocks.consentHost,
    start: mocks.start,
  },
}));

vi.mock("../../stores/global/ai-connection", () => ({
  aiConnectionStore: { status: mocks.connectionStatus },
}));

vi.mock("../../stores/global/config", () => ({
  configStore: { config: mocks.config },
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({
      editor: {
        currentBufferId: mocks.currentBufferId,
        getSelectionRange: mocks.getSelectionRange,
      },
    }),
  },
}));

import { runRewriteAction } from "../../commands/ai";

function endpoint(overrides: Record<string, unknown> = {}) {
  return {
    host: "api.deepseek.com",
    host_port: "api.deepseek.com",
    is_hosted: true,
    is_allowed: true,
    is_consented: true,
    key_state: { is_set: true, memory_only: false },
    ...overrides,
  };
}

beforeEach(() => {
  for (const m of Object.values(mocks)) m.mockReset?.();
  mocks.config.mockReturnValue({ ai: { model: "deepseek-chat" } });
  mocks.connectionStatus.mockReturnValue(null);
  mocks.currentBufferId.mockReturnValue("buf-1");
  mocks.getSelectionRange.mockReturnValue({
    from: 0,
    to: 5,
    text: "hello",
    usedSelection: true,
  });
  mocks.endpointState.mockResolvedValue(endpoint());
  mocks.consentHost.mockResolvedValue(endpoint({ is_consented: true }));
  mocks.requestConfirm.mockResolvedValue(true);
});

describe("nothing is sent until every blocker is cleared", () => {
  it("runs straight through when consent and key are already in place", async () => {
    await runRewriteAction("polish");
    expect(mocks.requestConfirm).not.toHaveBeenCalled();
    expect(mocks.start).toHaveBeenCalledTimes(1);
  });

  it("asks before the first send to a hosted provider, naming the host", async () => {
    mocks.endpointState.mockResolvedValue(endpoint({ is_consented: false }));
    await runRewriteAction("polish");
    const request = mocks.requestConfirm.mock.calls[0][0];
    expect(request.title).toContain("api.deepseek.com");
    expect(request.defaultAction).toBe("cancel");
    expect(mocks.consentHost).toHaveBeenCalledTimes(1);
    expect(mocks.start).toHaveBeenCalledTimes(1);
  });

  it("sends nothing when the user declines", async () => {
    mocks.endpointState.mockResolvedValue(endpoint({ is_consented: false }));
    mocks.requestConfirm.mockResolvedValue(false);
    await runRewriteAction("polish");
    expect(mocks.consentHost).not.toHaveBeenCalled();
    expect(mocks.start).not.toHaveBeenCalled();
  });

  it("sends nothing when consent cannot be recorded", async () => {
    mocks.endpointState.mockResolvedValue(endpoint({ is_consented: false }));
    mocks.consentHost.mockRejectedValue(new Error("disk full"));
    await runRewriteAction("polish");
    expect(mocks.start).not.toHaveBeenCalled();
  });

  it("stops at a missing key and offers settings", async () => {
    mocks.endpointState.mockResolvedValue(
      endpoint({ key_state: { is_set: false, memory_only: false } }),
    );
    await runRewriteAction("polish");
    expect(mocks.start).not.toHaveBeenCalled();
    expect(mocks.openSettings).toHaveBeenCalledWith("apps", "ai.api_key");
  });

  it("stops at a disallowed base URL", async () => {
    mocks.endpointState.mockResolvedValue(endpoint({ is_allowed: false }));
    await runRewriteAction("polish");
    expect(mocks.start).not.toHaveBeenCalled();
    expect(mocks.openSettings).toHaveBeenCalledWith("apps", "ai.base_url");
  });

  it("asks for the key before asking to send, so nobody agrees to a send that cannot happen", async () => {
    mocks.endpointState.mockResolvedValue(
      endpoint({ is_consented: false, key_state: { is_set: false, memory_only: false } }),
    );
    await runRewriteAction("polish");
    expect(mocks.requestConfirm).toHaveBeenCalledTimes(1);
    expect(mocks.requestConfirm.mock.calls[0][0].title).toBe("Add an API key for api.deepseek.com");
    expect(mocks.consentHost).not.toHaveBeenCalled();
    expect(mocks.openSettings).toHaveBeenCalledWith("apps", "ai.api_key");
    expect(mocks.start).not.toHaveBeenCalled();
  });

  it("asks one question for a whole document going to a host not yet allowed", async () => {
    mocks.endpointState.mockResolvedValue(endpoint({ is_consented: false }));
    mocks.consentHost.mockResolvedValue(endpoint({ is_consented: true }));
    mocks.getSelectionRange.mockReturnValue({
      from: 0,
      to: 120,
      text: "x".repeat(120),
      usedSelection: false,
    });
    await runRewriteAction("polish");
    expect(mocks.requestConfirm).toHaveBeenCalledTimes(1);
    const sheet = mocks.requestConfirm.mock.calls[0][0];
    expect(sheet.title).toBe("Send text to api.deepseek.com?");
    expect(sheet.message).toContain("the whole document (120 characters)");
    expect(mocks.consentHost).toHaveBeenCalledTimes(1);
    expect(mocks.start).toHaveBeenCalledTimes(1);
  });

  it("never reaches the endpoint check without a model", async () => {
    mocks.config.mockReturnValue({ ai: { model: "  " } });
    await runRewriteAction("polish");
    expect(mocks.endpointState).not.toHaveBeenCalled();
    expect(mocks.start).not.toHaveBeenCalled();
  });

  it("refuses an empty selection before asking anything", async () => {
    mocks.getSelectionRange.mockReturnValue({
      from: 0,
      to: 3,
      text: "   ",
      usedSelection: true,
    });
    await runRewriteAction("polish");
    expect(mocks.endpointState).not.toHaveBeenCalled();
    expect(mocks.start).not.toHaveBeenCalled();
  });

  it("asks nothing extra for a local endpoint", async () => {
    mocks.endpointState.mockResolvedValue(
      endpoint({
        host: "localhost",
        is_hosted: false,
        is_consented: true,
        key_state: { is_set: false, memory_only: false },
      }),
    );
    await runRewriteAction("polish");
    expect(mocks.requestConfirm).not.toHaveBeenCalled();
    expect(mocks.start).toHaveBeenCalledTimes(1);
  });

  it("asks once before sending to an address on the local network, with no key", async () => {
    const lan = {
      host: "192.168.1.20",
      host_port: "192.168.1.20:11434",
      is_hosted: true,
      is_private_network: true,
      key_state: { is_set: false, memory_only: false },
    };
    mocks.endpointState.mockResolvedValue(endpoint({ ...lan, is_consented: false }));
    mocks.consentHost.mockResolvedValue(endpoint({ ...lan, is_consented: true }));
    await runRewriteAction("polish");
    expect(mocks.requestConfirm).toHaveBeenCalledTimes(1);
    const sheet = mocks.requestConfirm.mock.calls[0][0];
    expect(sheet.title).toBe("Send text to 192.168.1.20:11434?");
    expect(sheet.message).toContain("local network");
    expect(sheet.message).not.toContain("API key");
    expect(mocks.consentHost).toHaveBeenCalledTimes(1);
    expect(mocks.start).toHaveBeenCalledTimes(1);

    mocks.requestConfirm.mockClear();
    mocks.endpointState.mockResolvedValue(endpoint({ ...lan, is_consented: true }));
    await runRewriteAction("polish");
    expect(mocks.requestConfirm).not.toHaveBeenCalled();
    expect(mocks.start).toHaveBeenCalledTimes(2);
  });

  it("rewrites with the on-device model with no model, key or consent to ask for", async () => {
    mocks.config.mockReturnValue({ ai: { provider: "apple", model: "" } });
    mocks.endpointState.mockResolvedValue(
      endpoint({
        host: null,
        host_port: null,
        is_hosted: false,
        is_allowed: false,
        is_consented: false,
        is_on_device: true,
        key_state: { is_set: false, memory_only: false },
      }),
    );
    await runRewriteAction("polish");
    expect(mocks.requestConfirm).not.toHaveBeenCalled();
    expect(mocks.start).toHaveBeenCalledTimes(1);

    mocks.getSelectionRange.mockReturnValue({
      from: 0,
      to: 40,
      text: "y".repeat(40),
      usedSelection: false,
    });
    await runRewriteAction("polish");
    expect(mocks.requestConfirm).toHaveBeenCalledTimes(1);
    expect(mocks.requestConfirm.mock.calls[0][0].title).toBe("Rewrite the whole document?");
  });

  it("names the local network rule when a base URL is turned away", async () => {
    mocks.endpointState.mockResolvedValue(endpoint({ is_allowed: false }));
    await runRewriteAction("polish");
    expect(mocks.requestConfirm.mock.calls[0][0].message).toContain("local network");
  });

  it("acts on a range captured earlier rather than re-reading the selection", async () => {
    const pinned = {
      from: 10,
      to: 20,
      text: "pinned text",
      usedSelection: true,
      bufferId: "buf-1",
    };
    await runRewriteAction("polish", pinned);
    expect(mocks.getSelectionRange).not.toHaveBeenCalled();
    expect(mocks.start).toHaveBeenCalledWith("polish", pinned);
  });
});
