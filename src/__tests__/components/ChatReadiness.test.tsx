import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import type { Readiness } from "../../stores/global/chat";

// The pane says what the connection still needs before a message is written,
// and every way of not being ready has one thing to press.

const mocks = vi.hoisted(() => ({
  readiness: vi.fn(),
  selectChatModel: vi.fn(),
  check: vi.fn(),
  openSettings: vi.fn(),
}));

vi.mock("../../stores/global/chat", async () => {
  const actual = await vi.importActual<typeof import("../../stores/global/chat")>(
    "../../stores/global/chat",
  );
  return { ...actual, chatStore: { readiness: mocks.readiness } };
});

vi.mock("../../stores/global/ai-connection", () => ({
  aiConnectionStore: { selectChatModel: mocks.selectChatModel, check: mocks.check },
}));

vi.mock("../../components/SettingsModal/SettingsModal", () => ({
  openSettings: mocks.openSettings,
}));

import ChatReadiness from "../../components/Chat/ChatReadiness";

const SETTINGS = { kind: "settings", section: "apps", setting: "ai.provider" } as const;

const CASES: { readiness: Readiness; text: string; action: string }[] = [
  {
    readiness: { state: "off", message: "Chat is turned off.", action: SETTINGS },
    text: "Chat is turned off.",
    action: "Open settings",
  },
  {
    readiness: { state: "no_model", message: "No model is set.", action: SETTINGS },
    text: "No model is set.",
    action: "Open settings",
  },
  {
    readiness: {
      state: "no_key",
      message: "Add an API key to use this connection.",
      action: SETTINGS,
    },
    text: "Add an API key to use this connection.",
    action: "Open settings",
  },
  {
    readiness: {
      state: "unconsented",
      message: "api.deepseek.com has not been allowed yet.",
      action: SETTINGS,
    },
    text: "api.deepseek.com has not been allowed yet.",
    action: "Open settings",
  },
  {
    readiness: {
      state: "offline_local",
      message: "Nothing is answering at localhost:11434.",
      action: { kind: "check" },
    },
    text: "Nothing is answering at localhost:11434.",
    action: "Retry detection",
  },
  {
    readiness: {
      state: "model_unavailable",
      message: "llama3 is not available on Ollama.",
      action: SETTINGS,
    },
    text: "llama3 is not available on Ollama.",
    action: "Use the connection's model",
  },
];

beforeEach(() => {
  mocks.readiness.mockReset();
  mocks.selectChatModel.mockReset().mockResolvedValue(undefined);
  mocks.check.mockReset().mockResolvedValue(undefined);
  mocks.openSettings.mockReset();
});

afterEach(() => {
  cleanup();
});

describe("the readiness line", () => {
  it("says nothing while the connection answers for itself", () => {
    mocks.readiness.mockReturnValue({ state: "ready" });
    const { container } = render(() => <ChatReadiness />);
    expect(container.querySelector(".chat-readiness")).toBeNull();
  });

  for (const found of CASES) {
    it(`offers one way out of ${found.readiness.state}`, () => {
      mocks.readiness.mockReturnValue(found.readiness);
      const { container, getByText } = render(() => <ChatReadiness />);

      expect(container.textContent).toContain(found.text);
      expect(getByText(found.action)).toBeTruthy();
      cleanup();
    });
  }

  it("puts the chat back on the connection's model", () => {
    mocks.readiness.mockReturnValue(CASES[5].readiness);
    const { getByText } = render(() => <ChatReadiness />);
    fireEvent.click(getByText("Use the connection's model"));
    expect(mocks.selectChatModel).toHaveBeenCalledWith(null);
  });

  it("looks for the local server again", () => {
    mocks.readiness.mockReturnValue(CASES[4].readiness);
    const { getByText } = render(() => <ChatReadiness />);
    fireEvent.click(getByText("Retry detection"));
    expect(mocks.check).toHaveBeenCalledTimes(1);
  });

  it("opens the setting the line is about", () => {
    mocks.readiness.mockReturnValue(CASES[1].readiness);
    const { getByText } = render(() => <ChatReadiness />);
    fireEvent.click(getByText("Open settings"));
    expect(mocks.openSettings).toHaveBeenCalledWith("apps", "ai.provider");
  });
});
