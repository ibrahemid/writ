import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import type { Readiness } from "../../stores/global/chat";

// The pane says what the connection still needs before a message is written,
// and every way of not being ready has one thing to press.

const mocks = vi.hoisted(() => ({
  readiness: vi.fn(),
  selectChatModel: vi.fn(),
  selectProvider: vi.fn(),
  probeLocal: vi.fn(),
  check: vi.fn(),
  openSettings: vi.fn(),
  setApiKey: vi.fn(),
  openConnectionControl: vi.fn(),
}));

vi.mock("../../stores/global/chat", async () => {
  const actual = await vi.importActual<typeof import("../../stores/global/chat")>(
    "../../stores/global/chat",
  );
  return { ...actual, chatStore: { readiness: mocks.readiness } };
});

vi.mock("../../stores/global/ai-connection", () => ({
  aiConnectionStore: {
    selectChatModel: mocks.selectChatModel,
    selectProvider: mocks.selectProvider,
    probeLocal: mocks.probeLocal,
    check: mocks.check,
  },
}));

vi.mock("../../stores/global/ai-rewrite", () => ({
  aiRewriteStore: { setApiKey: mocks.setApiKey },
}));

vi.mock("../../stores/global/ai-providers", () => ({
  aiProvidersStore: { byId: () => null },
}));

vi.mock("../../stores/global/config", () => ({
  configStore: { config: () => ({ ai: { provider: "anthropic" } }) },
}));

vi.mock("../../components/Chat/ChatConnectionControl", () => ({
  openConnectionControl: mocks.openConnectionControl,
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
    action: "Choose a model",
  },
  {
    readiness: {
      state: "no_key",
      message: "Add an API key to use this connection.",
      action: SETTINGS,
    },
    text: "Add an API key to use this connection.",
    action: "Save key",
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
  mocks.selectProvider.mockReset().mockResolvedValue(undefined);
  mocks.probeLocal.mockReset().mockResolvedValue({ ollama: false, lmstudio: false });
  mocks.setApiKey.mockReset().mockResolvedValue({ is_set: true, memory_only: false });
  mocks.openConnectionControl.mockReset();
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

  it("says what chat sends where before a model is chosen, and only then", () => {
    const explains = "Chat sends what you attach to a model you choose";
    mocks.readiness.mockReturnValue({ state: "no_model", message: "No model is set.", action: SETTINGS });
    const { container } = render(() => <ChatReadiness />);
    expect(container.textContent).toContain(explains);
    cleanup();
    mocks.readiness.mockReturnValue(CASES.find((c) => c.readiness.state === "no_key")!.readiness);
    const other = render(() => <ChatReadiness />);
    expect(other.container.textContent).not.toContain(explains);
  });

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
    mocks.readiness.mockReturnValue(CASES[3].readiness);
    const { getByText } = render(() => <ChatReadiness />);
    fireEvent.click(getByText("Open settings"));
    expect(mocks.openSettings).toHaveBeenCalledWith("apps", "ai.provider");
  });
});

describe("the first-use card", () => {
  it("offers a runtime running on this machine and the inline model picker", async () => {
    mocks.probeLocal.mockResolvedValue({ ollama: true, lmstudio: false });
    mocks.readiness.mockReturnValue(CASES[1].readiness);
    const { findByText, getByText, queryByText, container } = render(() => <ChatReadiness />);

    expect(container.textContent).toContain("Chat sends what you attach to a model you choose: one on this computer, or a provider with your key.");
    fireEvent.click(await findByText("Use Ollama"));
    await waitFor(() => expect(mocks.selectProvider).toHaveBeenCalledWith("ollama"));
    await waitFor(() => expect(mocks.check).toHaveBeenCalled());
    expect(queryByText("Use LM Studio")).toBeNull();

    fireEvent.click(getByText("Choose a model"));
    expect(mocks.openConnectionControl).toHaveBeenCalledTimes(1);
    expect(mocks.openSettings).not.toHaveBeenCalled();
  });

  it("offers llama.cpp and Jan when they are the runtimes running", async () => {
    mocks.probeLocal.mockResolvedValue({ ollama: false, lmstudio: false, llamacpp: true, jan: true });
    mocks.readiness.mockReturnValue(CASES[1].readiness);
    const { findByText, queryByText } = render(() => <ChatReadiness />);
    expect(await findByText("Use llama.cpp")).toBeTruthy();
    fireEvent.click(await findByText("Use Jan"));
    await waitFor(() => expect(mocks.selectProvider).toHaveBeenCalledWith("jan"));
    expect(queryByText("Use Ollama")).toBeNull();
  });

  it("takes the key on the card and checks the connection again", async () => {
    mocks.readiness.mockReturnValue(CASES[2].readiness);
    const { getByLabelText, getByText } = render(() => <ChatReadiness />);

    const field = getByLabelText("API key for anthropic") as HTMLInputElement;
    expect(field.type).toBe("password");
    fireEvent.input(field, { target: { value: "sk-typed" } });
    fireEvent.click(getByText("Save key"));

    await waitFor(() => expect(mocks.setApiKey).toHaveBeenCalledWith("anthropic", "sk-typed"));
    await waitFor(() => expect(mocks.check).toHaveBeenCalled());
    expect(field.value).toBe("");
    expect(mocks.openSettings).not.toHaveBeenCalled();
  });

  it("says so when the key could not be saved", async () => {
    mocks.setApiKey.mockRejectedValue(new Error("keychain locked"));
    mocks.readiness.mockReturnValue(CASES[2].readiness);
    const { getByLabelText, getByText, findByRole } = render(() => <ChatReadiness />);

    fireEvent.input(getByLabelText("API key for anthropic"), { target: { value: "sk-typed" } });
    fireEvent.click(getByText("Save key"));
    expect((await findByRole("alert")).textContent).toBe("The key was not saved.");
  });
});
