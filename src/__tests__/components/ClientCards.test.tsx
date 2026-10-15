import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@solidjs/testing-library";

import type { ClientApproval, McpClientCard, PendingClient } from "../../services/tauri";

// One card per MCP client (ADR-031 rule 2.4): Add runs the client's own
// command, opens its link or its bundle; Copy hands over the command or the
// JSON; Test counts the tools; the card follows the program from its first
// call to what it may do.

const h = vi.hoisted(() => ({
  mcpConnectCards: vi.fn(),
  mcpConnectAdd: vi.fn(),
  mcpConnectOpen: vi.fn(),
  mcpServerTest: vi.fn(),
  writeClipboardText: vi.fn().mockResolvedValue(undefined),
  setPermission: vi.fn().mockResolvedValue(undefined),
  deny: vi.fn().mockResolvedValue(undefined),
  undoDenial: vi.fn().mockResolvedValue(undefined),
}));

const lists = await vi.hoisted(async () => {
  const { createSignal: signal } = await import("solid-js");
  const [pending, setPending] = signal<PendingClient[]>([]);
  const [clients, setClients] = signal<ClientApproval[]>([]);
  const [caution, setCaution] = signal<string | undefined>(undefined);
  return { pending, setPending, clients, setClients, caution, setCaution };
});

vi.mock("../../services/tauri", () => ({
  mcpConnectCards: h.mcpConnectCards,
  mcpConnectAdd: h.mcpConnectAdd,
  mcpConnectOpen: h.mcpConnectOpen,
  mcpServerTest: h.mcpServerTest,
  writeClipboardText: h.writeClipboardText,
}));
vi.mock("../../stores/global/activity", () => ({
  activityStore: {
    pending: lists.pending,
    clients: lists.clients,
    commandCaution: lists.caution,
    setPermission: h.setPermission,
    deny: h.deny,
    undoDenial: h.undoDenial,
    isNotAllowed: () => false,
  },
}));

import ClientCards from "../../components/SettingsModal/apps/ClientCards";
import { runToastAction, toasts } from "../../stores/global/notifications";

const CARDS: McpClientCard[] = [
  {
    client: "claude-code",
    connection: "command",
    copy_text: 'claude mcp add --scope user writ -- "/usr/local/bin/writ" mcp',
    runs_command: true,
  },
  {
    client: "codex",
    connection: "command",
    copy_text: 'codex mcp add writ -- "/usr/local/bin/writ" mcp',
    runs_command: false,
  },
  { client: "gemini-cli", connection: "command", copy_text: "gemini mcp add", runs_command: false },
  { client: "cursor", connection: "link", copy_text: null, runs_command: false },
  { client: "vs-code", connection: "link", copy_text: null, runs_command: false },
  { client: "lm-studio", connection: "link", copy_text: null, runs_command: false },
  { client: "claude-desktop", connection: "bundle", copy_text: null, runs_command: false },
  { client: "other", connection: "json", copy_text: '{\n  "mcpServers": {}\n}', runs_command: false },
];

function waiting(name: string): PendingClient {
  return { name, version: "1.0.0", first_seen: "2026-10-15T09:00:00Z", last_seen: "2026-10-15T09:00:00Z", calls: 1 };
}

async function shown() {
  const result = render(() => <ClientCards />);
  await waitFor(() => expect(result.container.querySelectorAll("[data-client]")).toHaveLength(8));
  return result;
}

function card(container: HTMLElement, client: string): HTMLElement {
  return container.querySelector<HTMLElement>(`[data-client='${client}']`)!;
}

function text(element: Element | null): string {
  return element?.textContent?.trim() ?? "";
}

beforeEach(() => {
  h.mcpConnectCards.mockReset().mockResolvedValue(CARDS);
  h.mcpConnectAdd.mockReset().mockResolvedValue({ said: "Added" });
  h.mcpConnectOpen.mockReset().mockResolvedValue(undefined);
  h.mcpServerTest.mockReset();
  h.writeClipboardText.mockClear();
  lists.setPending([]);
  lists.setClients([]);
  lists.setCaution(undefined);
});

afterEach(() => cleanup());

describe("the client cards", () => {
  it("shows one card per client, named as a person knows it", async () => {
    const { container } = await shown();
    const names = Array.from(container.querySelectorAll(".client-card-name"), text);
    expect(names).toEqual([
      "Claude Code",
      "Codex",
      "Gemini CLI",
      "Cursor",
      "VS Code",
      "LM Studio",
      "Claude Desktop",
      "Other programs",
    ]);
  });

  it("runs a found client's own command, then waits for its first call", async () => {
    const { container } = await shown();
    const claude = card(container, "claude-code");
    expect(text(claude.querySelector(".client-card-access"))).toBe(
      "No access until you approve its first call.",
    );

    fireEvent.click(claude.querySelector("[data-action='mcp-card-add']")!);

    await waitFor(() =>
      expect(text(claude.querySelector(".client-card-access"))).toBe("Waiting for the first call."),
    );
    expect(h.mcpConnectAdd).toHaveBeenCalledWith("claude-code");
  });

  it("copies the command of a client that was not found, and has no Add", async () => {
    const { container } = await shown();
    const codex = card(container, "codex");
    expect(codex.querySelector("[data-action='mcp-card-add']")).toBeNull();
    expect(text(codex.querySelector(".client-card-how"))).toBe(
      "Not found on this computer. Run this in a terminal.",
    );
    expect(text(codex.querySelector("[data-copy-text]"))).toBe(CARDS[1].copy_text);

    fireEvent.click(codex.querySelector("[data-action='mcp-card-copy']")!);

    await waitFor(() => expect(h.writeClipboardText).toHaveBeenCalledWith(CARDS[1].copy_text));
    await waitFor(() =>
      expect(text(codex.querySelector(".client-card-access"))).toBe("Waiting for the first call."),
    );
  });

  it("opens the link for Cursor and the bundle for Claude Desktop", async () => {
    const { container } = await shown();

    fireEvent.click(card(container, "cursor").querySelector("[data-action='mcp-card-add']")!);
    fireEvent.click(card(container, "claude-desktop").querySelector("[data-action='mcp-card-add']")!);

    await waitFor(() => expect(h.mcpConnectOpen).toHaveBeenCalledTimes(2));
    expect(h.mcpConnectOpen).toHaveBeenNthCalledWith(1, "cursor");
    expect(h.mcpConnectOpen).toHaveBeenNthCalledWith(2, "claude-desktop");
  });

  it("says why an Add failed, in the client's card", async () => {
    h.mcpConnectOpen.mockRejectedValue("No app on this computer opened the link. Install the program first.");
    const { container } = await shown();
    const cursor = card(container, "cursor");

    fireEvent.click(cursor.querySelector("[data-action='mcp-card-add']")!);

    await waitFor(() =>
      expect(text(cursor.querySelector("[data-result='add']"))).toBe(
        "No app on this computer opened the link. Install the program first.",
      ),
    );
    expect(text(cursor.querySelector(".client-card-access"))).toBe(
      "No access until you approve its first call.",
    );
  });

  it("shows what a client's own command said when it failed", async () => {
    const said = "The claude command could not add Writ: MCP server writ already exists in user config.";
    h.mcpConnectAdd.mockRejectedValue(said);
    const { container } = await shown();
    const claude = card(container, "claude-code");

    fireEvent.click(claude.querySelector("[data-action='mcp-card-add']")!);

    await waitFor(() => expect(text(claude.querySelector("[data-result='add']"))).toBe(said));
  });

  it("reports how many tools the server listed", async () => {
    h.mcpServerTest.mockResolvedValue({ tools: 14 });
    const { container } = await shown();
    const vscode = card(container, "vs-code");

    fireEvent.click(vscode.querySelector("[data-action='mcp-card-test']")!);

    await waitFor(() =>
      expect(text(vscode.querySelector("[data-result='test']"))).toBe("Writ answered with 14 tools."),
    );
  });

  it("passes on why the test failed", async () => {
    h.mcpServerTest.mockRejectedValue("Writ's server did not answer within 10 seconds.");
    const { container } = await shown();
    const other = card(container, "other");

    fireEvent.click(other.querySelector("[data-action='mcp-card-test']")!);

    await waitFor(() =>
      expect(text(other.querySelector("[data-result='test']"))).toBe(
        "Writ's server did not answer within 10 seconds.",
      ),
    );
  });

  it("puts the approval choices on the card of the program that is waiting", async () => {
    lists.setPending([waiting("claude-code")]);
    const { container } = await shown();
    const claude = card(container, "claude-code");

    expect(text(claude.querySelector(".client-card-access"))).toBe(
      "Waiting for your approval. No access until then.",
    );
    fireEvent.click(claude.querySelector("[data-action='approve-read']")!);

    await waitFor(() => expect(h.setPermission).toHaveBeenCalledWith("claude-code", true, false));
    expect(card(container, "cursor").querySelector("[data-action='approve-read']")).toBeNull();
  });

  it("offers Undo for a program turned down from its card", async () => {
    lists.setPending([waiting("cursor-vscode")]);
    const { container } = await shown();
    const cursor = card(container, "cursor");

    fireEvent.click(cursor.querySelector("[data-action='refuse']")!);

    await waitFor(() => expect(h.deny).toHaveBeenCalledWith("cursor-vscode"));
    const notice = toasts().find((toast) => toast.text === "Cursor is not allowed.");
    expect(notice?.action?.label).toBe("Undo");
    runToastAction(notice!.id);
    await waitFor(() => expect(h.undoDenial).toHaveBeenCalledWith("cursor-vscode"));
  });

  it("says what a decided program may do, by the name it sends", async () => {
    lists.setClients([
      { name: "cursor-vscode", first_seen: "2026-10-15T09:00:00Z", read: true, write: false, context: false },
      { name: "claude-ai", first_seen: "2026-10-15T09:00:00Z", read: true, write: true, context: false },
      { name: "codex-mcp-client", first_seen: "2026-10-15T09:00:00Z", read: false, write: false, context: false },
    ]);
    const { container } = await shown();

    expect(text(card(container, "cursor").querySelector(".client-card-access"))).toBe(
      "Can read the files in your folder. Writing is off.",
    );
    expect(text(card(container, "claude-desktop").querySelector(".client-card-access"))).toBe(
      "Can read and write the files in your folder.",
    );
    expect(text(card(container, "codex").querySelector(".client-card-access"))).toBe(
      "Not allowed to read or write your files.",
    );
  });

  it("holds every action while the command is not ready to hand out", async () => {
    lists.setCaution("Move Writ to your Applications folder and open it from there first.");
    const { container } = await shown();

    const buttons = Array.from(container.querySelectorAll<HTMLButtonElement>("[data-action^='mcp-card-']"));
    expect(buttons.length).toBeGreaterThan(0);
    expect(buttons.every((button) => button.disabled)).toBe(true);
  });

  it("says so when the cards could not be listed on the first read", async () => {
    h.mcpConnectCards.mockRejectedValue(new Error("stopped"));
    vi.resetModules();
    const { default: FreshCards } = await import("../../components/SettingsModal/apps/ClientCards");
    const { container } = render(() => <FreshCards />);
    await waitFor(() =>
      expect(text(container.querySelector(".settings-programs-none"))).toBe(
        "The programs could not be listed.",
      ),
    );
  });
});
