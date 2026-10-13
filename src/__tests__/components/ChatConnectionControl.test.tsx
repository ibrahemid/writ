import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";

// One control names the connection a reply will come from, and changing it
// here changes it everywhere: the pane and the settings panel read the same
// catalog, so a model from a provider that is no longer current is never on
// offer.

const mocks = vi.hoisted(() => ({
  config: vi.fn(),
  catalog: vi.fn(),
  listing: vi.fn(),
  selectProvider: vi.fn(),
  selectChatModel: vi.fn(),
  probeLocal: vi.fn(),
  hasApiKey: vi.fn(),
  rows: vi.fn(),
  openSettings: vi.fn(),
}));

const PROVIDERS = [
  {
    id: "ollama",
    label: "Ollama",
    group: "local",
    needs_key: false,
    default_model: "qwen3:8b",
    curated_models: ["qwen3:8b"],
  },
  {
    id: "deepseek",
    label: "DeepSeek",
    group: "hosted",
    needs_key: true,
    default_model: "deepseek-chat",
    curated_models: ["deepseek-chat", "deepseek-reasoner"],
  },
];

vi.mock("../../stores/global/config", () => ({
  configStore: { config: mocks.config },
}));

vi.mock("../../stores/global/ai-connection", () => ({
  aiConnectionStore: {
    catalog: mocks.catalog,
    listing: mocks.listing,
    selectProvider: mocks.selectProvider,
    selectChatModel: mocks.selectChatModel,
    probeLocal: mocks.probeLocal,
  },
  modelListDisplay: (error: { kind: string }, host: string) => ({
    text: `Could not reach ${host}`,
    tone: "error",
    kind: error.kind,
  }),
}));

vi.mock("../../stores/global/ai-providers", () => ({
  aiProvidersStore: {
    rows: mocks.rows,
    byId: (id: string) => mocks.rows().find((row: { id: string }) => row.id === id) ?? null,
    load: () => Promise.resolve(),
    groupOf: (id: string) =>
      mocks.rows().find((row: { id: string }) => row.id === id)?.group ?? null,
  },
}));

vi.mock("../../stores/global/ai-rewrite", () => ({
  aiRewriteStore: { hasApiKey: mocks.hasApiKey },
}));

vi.mock("../../components/SettingsModal/SettingsModal", () => ({
  openSettings: mocks.openSettings,
}));

import ChatConnectionControl from "../../components/Chat/ChatConnectionControl";

function config(provider: string, model: string, chatModel = "", chatProvider = "") {
  return {
    ai: {
      provider,
      model,
      base_url: "",
      chat: { enabled: true, model: chatModel, model_provider: chatProvider },
    },
  };
}

async function openMenu() {
  const view = render(() => <ChatConnectionControl />);
  fireEvent.click(view.container.querySelector(".chat-connection") as HTMLElement);
  await waitFor(() => expect(view.container.querySelector(".chat-connection-menu")).toBeTruthy());
  return view;
}

function rowLabels(container: HTMLElement): string[] {
  return Array.from(container.querySelectorAll(".chat-menu-row")).map(
    (row) => row.textContent ?? "",
  );
}

beforeEach(() => {
  mocks.config.mockReset().mockReturnValue(config("ollama", "qwen3:8b"));
  mocks.catalog
    .mockReset()
    .mockReturnValue({ provider: "ollama", models: ["qwen3:8b", "phi4"], source: "live", error: null });
  mocks.listing.mockReset().mockReturnValue(false);
  mocks.selectProvider.mockReset().mockResolvedValue(undefined);
  mocks.selectChatModel.mockReset().mockResolvedValue(undefined);
  mocks.probeLocal.mockReset().mockResolvedValue({ ollama: true, lmstudio: false });
  mocks.hasApiKey.mockReset().mockResolvedValue({ is_set: false, memory_only: false });
  mocks.rows.mockReset().mockReturnValue(PROVIDERS);
  mocks.openSettings.mockReset();
});

afterEach(() => {
  cleanup();
});

describe("the connection control", () => {
  it("names the provider and the model it will answer with", () => {
    const { container } = render(() => <ChatConnectionControl />);
    expect(container.querySelector(".chat-connection")?.textContent).toContain("Ollama");
    expect(container.querySelector(".chat-connection")?.textContent).toContain("qwen3:8b");
  });

  it("offers no model from another provider", async () => {
    mocks.config.mockReturnValue(config("deepseek", "deepseek-chat"));
    // A list read for Ollama is not a list of what DeepSeek runs, and the
    // store already drops it; nothing here may put it back on offer.
    mocks.catalog.mockReturnValue(null);
    const { container } = await openMenu();

    const labels = rowLabels(container);
    expect(labels.some((text) => text.includes("phi4"))).toBe(false);
    expect(labels.some((text) => text.includes("deepseek-reasoner"))).toBe(true);
    expect(labels.some((text) => text.includes("suggested"))).toBe(true);
  });

  it("shows a saved model the provider does not list as unavailable", async () => {
    mocks.config.mockReturnValue(config("ollama", "qwen3:8b", "llama3:70b", "ollama"));
    const { container } = await openMenu();

    const row = Array.from(container.querySelectorAll(".chat-menu-row")).find((el) =>
      (el.textContent ?? "").includes("llama3:70b"),
    ) as HTMLButtonElement;
    expect(row.textContent).toContain("not available on Ollama");
    expect(row.disabled).toBe(true);
  });

  it("says the list is being read, that it was empty, and why it failed", async () => {
    mocks.listing.mockReturnValue(true);
    mocks.catalog.mockReturnValue(null);
    const reading = await openMenu();
    expect(reading.container.textContent).toContain("Reading the model list.");
    cleanup();

    mocks.listing.mockReturnValue(false);
    mocks.catalog.mockReturnValue({ provider: "ollama", models: [], source: "live", error: null });
    const empty = await openMenu();
    expect(empty.container.textContent).toContain("Ollama listed no models.");
    cleanup();

    mocks.catalog.mockReturnValue({
      provider: "ollama",
      models: [],
      source: "none",
      error: { kind: "unreachable" },
    });
    const failed = await openMenu();
    expect(failed.container.textContent).toContain("Could not reach Ollama");
  });

  it("switches provider through the store", async () => {
    const { container } = await openMenu();
    const row = Array.from(container.querySelectorAll(".chat-menu-row")).find((el) =>
      (el.textContent ?? "").includes("DeepSeek"),
    ) as HTMLElement;

    fireEvent.click(row);
    await waitFor(() => expect(mocks.selectProvider).toHaveBeenCalledWith("deepseek"));
    expect(mocks.selectChatModel).not.toHaveBeenCalled();
  });

  // A provider whose own list holds none of its row's ids seeds no model, so
  // the list opens again on the choice a send still needs.
  it("opens again when the chosen provider's list seeds no model", async () => {
    mocks.selectProvider.mockImplementation(async () => {
      mocks.config.mockReturnValue(config("deepseek", ""));
      mocks.catalog.mockReturnValue({
        provider: "deepseek",
        models: ["deepseek-x2", "deepseek-x3"],
        source: "live",
        error: null,
      });
    });
    const { container } = await openMenu();
    const row = Array.from(container.querySelectorAll(".chat-menu-row")).find((item) =>
      item.textContent?.includes("DeepSeek"),
    ) as HTMLElement;
    fireEvent.click(row);
    expect(container.querySelector(".chat-connection-menu")).toBeNull();

    await waitFor(() => expect(mocks.selectProvider).toHaveBeenCalledWith("deepseek"));
    await waitFor(() => expect(container.querySelector(".chat-connection-menu")).toBeTruthy());
  });

  it("puts focus on the first model when it opens again for one", async () => {
    // Read through signals, so the rows rendered on reopening are the new
    // provider's, as the stores make them in the app.
    const [held, setHeld] = createSignal({
      config: config("ollama", "qwen3:8b"),
      catalog: { provider: "ollama", models: ["qwen3:8b", "phi4"], source: "live", error: null },
    });
    mocks.config.mockImplementation(() => held().config);
    mocks.catalog.mockImplementation(() => held().catalog);
    mocks.selectProvider.mockImplementation(async () => {
      setHeld({
        config: config("deepseek", ""),
        catalog: {
          provider: "deepseek",
          models: ["deepseek-x2", "deepseek-x3"],
          source: "live",
          error: null,
        },
      });
    });
    const { container } = await openMenu();
    const row = Array.from(container.querySelectorAll(".chat-menu-row")).find((item) =>
      item.textContent?.includes("DeepSeek"),
    ) as HTMLElement;
    fireEvent.click(row);

    await waitFor(() => expect(container.querySelector(".chat-connection-menu")).toBeTruthy());
    await waitFor(() => expect(document.activeElement?.textContent).toContain("deepseek-x2"));
  });

  // The list can land after the person has gone on to the composer; the keys
  // they type there must not reach a menu that opened under them.
  it("stays closed when focus has left the control before the list lands", async () => {
    const composer = document.createElement("textarea");
    document.body.appendChild(composer);
    try {
      mocks.selectProvider.mockImplementation(async () => {
        composer.focus();
        mocks.config.mockReturnValue(config("deepseek", ""));
        mocks.catalog.mockReturnValue({
          provider: "deepseek",
          models: ["deepseek-x2", "deepseek-x3"],
          source: "live",
          error: null,
        });
      });
      const { container } = await openMenu();
      const row = Array.from(container.querySelectorAll(".chat-menu-row")).find((item) =>
        item.textContent?.includes("DeepSeek"),
      ) as HTMLElement;
      fireEvent.click(row);

      await waitFor(() => expect(mocks.selectProvider).toHaveBeenCalledWith("deepseek"));
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(container.querySelector(".chat-connection-menu")).toBeNull();
      expect(document.activeElement).toBe(composer);
    } finally {
      composer.remove();
    }
  });

  it("stays closed when the chosen provider's list seeds a model", async () => {
    mocks.selectProvider.mockImplementation(async () => {
      mocks.config.mockReturnValue(config("deepseek", ""));
      mocks.catalog.mockReturnValue({
        provider: "deepseek",
        models: ["deepseek-x2", "deepseek-chat"],
        source: "live",
        error: null,
      });
    });
    const { container } = await openMenu();
    const row = Array.from(container.querySelectorAll(".chat-menu-row")).find((item) =>
      item.textContent?.includes("DeepSeek"),
    ) as HTMLElement;
    fireEvent.click(row);

    await waitFor(() => expect(mocks.selectProvider).toHaveBeenCalledWith("deepseek"));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(container.querySelector(".chat-connection-menu")).toBeNull();
  });

  it("clears the override when the connection's own model is picked", async () => {
    mocks.config.mockReturnValue(config("ollama", "qwen3:8b", "phi4", "ollama"));
    const { container } = await openMenu();
    const rows = Array.from(container.querySelectorAll(".chat-menu-row"));

    fireEvent.click(rows.find((el) => (el.textContent ?? "").includes("qwen3:8b")) as HTMLElement);
    await waitFor(() => expect(mocks.selectChatModel).toHaveBeenCalledWith(null));

    cleanup();
    const again = await openMenu();
    fireEvent.click(
      Array.from(again.container.querySelectorAll(".chat-menu-row")).find((el) =>
        (el.textContent ?? "").includes("phi4"),
      ) as HTMLElement,
    );
    await waitFor(() => expect(mocks.selectChatModel).toHaveBeenCalledWith("phi4"));
  });

  it("shows what each provider row still needs", async () => {
    const { container } = await openMenu();
    await waitFor(() => expect(container.textContent).toContain("Running"));
    expect(container.textContent).toContain("No key");
  });

  it("opens the settings section from the last row", async () => {
    const { getByText } = await openMenu();
    fireEvent.click(getByText("AI settings"));
    expect(mocks.openSettings).toHaveBeenCalledWith("apps", "ai.provider");
  });

  it("walks the rows and gives focus back on Escape", async () => {
    const { container } = await openMenu();
    const menu = container.querySelector(".chat-connection-menu") as HTMLElement;
    const button = container.querySelector(".chat-connection") as HTMLElement;

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement?.classList.contains("chat-menu-row")).toBe(true);

    fireEvent.keyDown(menu, { key: "Escape" });
    await waitFor(() => expect(container.querySelector(".chat-connection-menu")).toBeNull());
    expect(document.activeElement).toBe(button);
  });
});
