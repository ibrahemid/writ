import { describe, it, expect, vi, beforeEach } from "vitest";

// What the pane settles before a note leaves the machine: the switch, the
// endpoint, the model, the host's consent and the key, in one pass. The dialog
// that asks names the host, how many notes and how many bytes, and says
// nothing else.

const mocks = vi.hoisted(() => ({
  requestConfirm: vi.fn(),
  showToast: vi.fn(),
  openSettings: vi.fn(),
  endpointState: vi.fn(),
  send: vi.fn(),
  attachments: vi.fn<() => { path: string; name: string; bytes: number }[]>(() => []),
  attachedOnDisk:
    vi.fn<
      () => Promise<
        { path: string; name: string; bytes: number; state?: string; reason?: string }[]
      >
    >(),
  setAttachedList: vi.fn(),
  catalog: vi.fn<() => { provider: string; models: string[]; source: string } | null>(() => null),
  draft: vi.fn(() => "what does it argue"),
  consentHost: vi.fn(),
  config: vi.fn(),
  registerCommand: vi.fn(),
  unregisterCommand: vi.fn(),
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

vi.mock("../../stores/global/chat", () => ({
  chatStore: {
    endpointState: mocks.endpointState,
    attachments: mocks.attachments,
    attachedOnDisk: mocks.attachedOnDisk,
    draft: mocks.draft,
    send: mocks.send,
    setAttachedList: mocks.setAttachedList,
  },
  totalBytes: (notes: { bytes: number }[]) => notes.reduce((sum, n) => sum + n.bytes, 0),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: { config: mocks.config },
}));

// Consent goes through the connection both features share: the command is
// called once and the config it wrote is re-read there.
vi.mock("../../stores/global/ai-connection", () => ({
  aiConnectionStore: { catalog: mocks.catalog, consentHost: mocks.consentHost },
}));

vi.mock("../../services/tauri", () => ({
  aiConsentHost: mocks.consentHost,
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => null },
}));

vi.mock("../../commands/registry", () => ({
  registerCommand: mocks.registerCommand,
  unregisterCommand: mocks.unregisterCommand,
}));

import {
  byteLabel,
  clearBlockersBeforeSending,
  sendChatMessage,
  sendNotice,
  toggleChat,
} from "../../commands/chat";

function endpoint(overrides: Record<string, unknown> = {}) {
  return {
    enabled: true,
    provider: "anthropic",
    model: "a-model",
    host: "api.example.com",
    host_port: "api.example.com",
    is_hosted: true,
    is_allowed: true,
    is_consented: true,
    key_state: { is_set: true, memory_only: false },
    ...overrides,
  };
}

const NOTES = [
  { path: "/notes/Launch.md", name: "Launch.md", bytes: 8 * 1024 },
  { path: "/notes/Other.md", name: "Other.md", bytes: 6 * 1024 },
];

beforeEach(() => {
  mocks.requestConfirm.mockReset().mockResolvedValue(true);
  mocks.showToast.mockReset();
  mocks.openSettings.mockReset();
  mocks.endpointState.mockReset().mockResolvedValue(endpoint());
  mocks.consentHost.mockReset().mockResolvedValue(endpoint());
  mocks.send.mockReset().mockResolvedValue(undefined);
  mocks.attachments.mockReturnValue(NOTES);
  mocks.attachedOnDisk.mockReset().mockResolvedValue(NOTES);
  mocks.setAttachedList.mockReset();
  mocks.catalog.mockReset().mockReturnValue(null);
  mocks.registerCommand.mockReset();
  mocks.unregisterCommand.mockReset();
});

describe("what the send dialog says", () => {
  it("names the host, how many notes and how many bytes", () => {
    const notice = sendNotice("api.example.com", NOTES);
    expect(notice.title).toBe("Send notes to api.example.com?");
    expect(notice.message).toContain("2 notes");
    expect(notice.message).toContain("14 KB");
    expect(notice.message).toContain("api.example.com");
  });

  it("counts one note as one note", () => {
    expect(sendNotice("localhost", [NOTES[0]]).message).toContain("1 note (8 KB)");
  });

  it("says a size in the unit that reads", () => {
    expect(byteLabel(512)).toBe("512 bytes");
    expect(byteLabel(2048)).toBe("2 KB");
    expect(byteLabel(3 * 1024 * 1024)).toBe("3.0 MB");
  });
});

describe("the blockers before a send", () => {
  it("asks about the host once and records the answer for that host", async () => {
    mocks.endpointState
      .mockResolvedValueOnce(endpoint({ is_consented: false }))
      .mockResolvedValueOnce(endpoint({ is_consented: true }));

    expect(await clearBlockersBeforeSending(NOTES)).toBe(true);
    expect(mocks.consentHost).toHaveBeenCalledWith();
    const asked = mocks.requestConfirm.mock.calls[0][0];
    expect(asked.title).toBe("Send notes to api.example.com?");
    expect(asked.message).toContain("2 notes");
    expect(asked.confirmLabel).toBe("Send");
  });

  it("sends nothing when the question is answered no", async () => {
    mocks.endpointState.mockResolvedValue(endpoint({ is_consented: false }));
    mocks.requestConfirm.mockResolvedValue(false);

    expect(await clearBlockersBeforeSending(NOTES)).toBe(false);
    expect(mocks.consentHost).not.toHaveBeenCalled();
  });

  it("stops on a switch that is off", async () => {
    mocks.endpointState.mockResolvedValue(endpoint({ enabled: false }));
    expect(await clearBlockersBeforeSending(NOTES)).toBe(false);
    expect(mocks.openSettings).toHaveBeenCalledWith("ai", "ai.chat.enabled");
  });

  it("stops on a base URL the guard refuses", async () => {
    mocks.endpointState.mockResolvedValue(endpoint({ is_allowed: false }));
    expect(await clearBlockersBeforeSending(NOTES)).toBe(false);
    expect(mocks.openSettings).toHaveBeenCalledWith("ai", "ai.provider");
  });

  it("stops when no model is set", async () => {
    mocks.endpointState.mockResolvedValue(endpoint({ model: "  " }));
    expect(await clearBlockersBeforeSending(NOTES)).toBe(false);
    expect(mocks.openSettings).toHaveBeenCalledWith("ai", "ai.model");
  });

  it("stops when a hosted endpoint has no key", async () => {
    mocks.endpointState.mockResolvedValue(
      endpoint({ key_state: { is_set: false, memory_only: false } }),
    );
    expect(await clearBlockersBeforeSending(NOTES)).toBe(false);
    expect(mocks.openSettings).toHaveBeenCalledWith("ai", "ai.api_key");
  });

  it("asks nothing of a local endpoint", async () => {
    mocks.endpointState.mockResolvedValue(
      endpoint({
        host: "localhost",
        is_hosted: false,
        key_state: { is_set: false, memory_only: false },
      }),
    );
    expect(await clearBlockersBeforeSending(NOTES)).toBe(true);
    expect(mocks.requestConfirm).not.toHaveBeenCalled();
  });

  it("asks about the sizes on disk, not the sizes the tabs recorded", async () => {
    // Only that the dialog is built from the refreshed list. What that list
    // holds is the mapping's job, asserted against the real code in
    // src/__tests__/stores/chat-attached-sizes.test.ts.
    mocks.attachedOnDisk.mockResolvedValue([
      { path: "/notes/Launch.md", name: "Launch.md", bytes: 16 * 1024 },
      NOTES[1],
    ]);
    mocks.endpointState.mockResolvedValue(endpoint({ is_consented: false }));
    await sendChatMessage();
    const asked = mocks.requestConfirm.mock.calls[0][0];
    expect(asked.message).toContain("22 KB");
  });

  it("sends nothing when the attached notes cannot be read", async () => {
    // A note that cannot be read comes back as one marked chip carrying the
    // reason, so the pane can name the note rather than failing the whole list.
    mocks.attachedOnDisk.mockResolvedValue([
      {
        path: "/elsewhere/Launch.md",
        name: "Launch.md",
        bytes: 0,
        state: "unreadable",
        reason: "Launch.md is outside your notes folder.",
      },
    ]);
    await sendChatMessage();
    expect(mocks.requestConfirm).not.toHaveBeenCalled();
    expect(mocks.send).not.toHaveBeenCalled();
    expect(mocks.showToast).toHaveBeenCalledWith(
      "Launch.md is outside your notes folder.",
      "error",
    );
  });

  it("stops when the chosen model is not in the catalog", async () => {
    // A live catalog is the provider's own inventory, so a model missing from
    // one cannot answer and the send is stopped before anything is asked.
    mocks.catalog.mockReturnValue({
      provider: "anthropic",
      models: ["another-model"],
      source: "live",
    });
    mocks.endpointState.mockResolvedValue(endpoint());

    const cleared = await clearBlockersBeforeSending([]);

    expect(cleared).toBe(false);
    const asked = mocks.requestConfirm.mock.calls[0][0];
    expect(asked.title).toContain("a-model");
    expect(asked.message).toContain("anthropic");
  });

  it("lets a curated list through, because it is not an inventory", async () => {
    mocks.catalog.mockReturnValue({
      provider: "anthropic",
      models: ["another-model"],
      source: "curated",
    });
    mocks.endpointState.mockResolvedValue(endpoint());

    expect(await clearBlockersBeforeSending([])).toBe(true);
  });

  it("sends only after the blockers are cleared", async () => {
    mocks.endpointState.mockResolvedValue(endpoint({ enabled: false }));
    await sendChatMessage();
    expect(mocks.send).not.toHaveBeenCalled();

    mocks.endpointState.mockResolvedValue(endpoint());
    await sendChatMessage();
    expect(mocks.send).toHaveBeenCalledTimes(1);
  });
});

describe("the pane's own command", () => {
  it("opens the settings that would give it a pane when chat is off", async () => {
    mocks.config.mockReturnValue({ ai: { chat: { enabled: false } } });
    mocks.requestConfirm.mockResolvedValue(true);

    await toggleChat();

    expect(mocks.requestConfirm.mock.calls[0][0].title).toBe("Chat is turned off");
    expect(mocks.openSettings).toHaveBeenCalledWith("ai", "ai.chat.enabled");
  });

  it("asks nothing when chat is on", async () => {
    mocks.config.mockReturnValue({ ai: { chat: { enabled: true } } });

    await toggleChat();

    expect(mocks.requestConfirm).not.toHaveBeenCalled();
    expect(mocks.openSettings).not.toHaveBeenCalled();
  });
});
