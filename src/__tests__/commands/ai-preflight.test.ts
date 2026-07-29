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

vi.mock("../../components/ConfirmDialog/ConfirmDialog", () => ({
  requestConfirm: mocks.requestConfirm,
}));

vi.mock("../../components/Notifications/Toast", () => ({
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
    expect(mocks.openSettings).toHaveBeenCalledWith("ai", "ai.api_key");
  });

  it("stops at a disallowed base URL", async () => {
    mocks.endpointState.mockResolvedValue(endpoint({ is_allowed: false }));
    await runRewriteAction("polish");
    expect(mocks.start).not.toHaveBeenCalled();
    expect(mocks.openSettings).toHaveBeenCalledWith("ai", "ai.base_url");
  });

  it("asks for consent and the key in one pass, not two failures", async () => {
    // Missing both: consent is asked first, then the key is resolved from the
    // state the consent call returned.
    mocks.endpointState.mockResolvedValue(
      endpoint({ is_consented: false, key_state: { is_set: false, memory_only: false } }),
    );
    mocks.consentHost.mockResolvedValue(
      endpoint({ is_consented: true, key_state: { is_set: false, memory_only: false } }),
    );
    await runRewriteAction("polish");
    expect(mocks.consentHost).toHaveBeenCalledTimes(1);
    expect(mocks.openSettings).toHaveBeenCalledWith("ai", "ai.api_key");
    expect(mocks.start).not.toHaveBeenCalled();
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
