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
  attachedOnDisk: vi.fn<() => Promise<{ path: string; name: string; bytes: number }[]>>(),
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
  },
  totalBytes: (notes: { bytes: number }[]) => notes.reduce((sum, n) => sum + n.bytes, 0),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: { config: mocks.config },
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
  syncChatCommands,
  CHAT_TOGGLE_COMMAND_ID,
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
    expect(mocks.consentHost).toHaveBeenCalledWith("chat");
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
    expect(mocks.openSettings).toHaveBeenCalledWith("ai", "ai.chat_enabled");
  });

  it("stops on a base URL the guard refuses", async () => {
    mocks.endpointState.mockResolvedValue(endpoint({ is_allowed: false }));
    expect(await clearBlockersBeforeSending(NOTES)).toBe(false);
    expect(mocks.openSettings).toHaveBeenCalledWith("ai", "ai.chat_base_url");
  });

  it("stops when no model is set", async () => {
    mocks.endpointState.mockResolvedValue(endpoint({ model: "  " }));
    expect(await clearBlockersBeforeSending(NOTES)).toBe(false);
    expect(mocks.openSettings).toHaveBeenCalledWith("ai", "ai.chat_model");
  });

  it("stops when a hosted endpoint has no key", async () => {
    mocks.endpointState.mockResolvedValue(
      endpoint({ key_state: { is_set: false, memory_only: false } }),
    );
    expect(await clearBlockersBeforeSending(NOTES)).toBe(false);
    expect(mocks.openSettings).toHaveBeenCalledWith("ai", "ai.chat_api_key");
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

  it("counts the bytes the file holds, not the bytes a tab recorded", async () => {
    // The tab read Launch.md at 8 KB; another program has since doubled it.
    // The number a person agrees to has to be the number that is sent.
    mocks.attachedOnDisk.mockResolvedValue([
      { path: "/notes/Launch.md", name: "Launch.md", bytes: 16 * 1024 },
      NOTES[1],
    ]);
    mocks.endpointState.mockResolvedValue(endpoint({ is_consented: false }));
    await sendChatMessage();
    const asked = mocks.requestConfirm.mock.calls[0][0];
    expect(asked.message).toContain("22 KB");
    expect(asked.message).not.toContain("14 KB");
  });

  it("sends nothing when the attached notes cannot be read", async () => {
    mocks.attachedOnDisk.mockRejectedValue("gone");
    await sendChatMessage();
    expect(mocks.requestConfirm).not.toHaveBeenCalled();
    expect(mocks.send).not.toHaveBeenCalled();
    expect(mocks.showToast).toHaveBeenCalled();
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

describe("the palette entry", () => {
  it("exists only while the setting is on", () => {
    mocks.config.mockReturnValue({ ai: { chat: { enabled: true } } });
    syncChatCommands();
    expect(mocks.registerCommand).toHaveBeenCalledTimes(1);
    expect(mocks.registerCommand.mock.calls[0][0].id).toBe(CHAT_TOGGLE_COMMAND_ID);

    mocks.config.mockReturnValue({ ai: { chat: { enabled: false } } });
    syncChatCommands();
    expect(mocks.unregisterCommand).toHaveBeenCalledWith(CHAT_TOGGLE_COMMAND_ID);
  });

  it("is not registered while the setting is off", () => {
    mocks.config.mockReturnValue({ ai: { chat: { enabled: false } } });
    syncChatCommands();
    expect(mocks.registerCommand).not.toHaveBeenCalled();
  });
});
