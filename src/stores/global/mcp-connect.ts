import { createSignal } from "solid-js";

import {
  mcpConnectAdd,
  mcpConnectCards,
  mcpConnectOpen,
  mcpServerTest,
  writeClipboardText,
  type McpClientCard,
  type McpClientId,
} from "../../services/tauri";
import { userErrorText } from "../../lib/error-text";
import { isClientsName } from "../../lib/mcp-clients";
import { logFailure } from "../../lib/log";
import { activityStore, type ClientApproval, type PendingClient } from "./activity";

export type { McpClientCard, McpClientId };

/** What the card's Add or Copy did last. */
export type CardAction =
  | { phase: "idle" }
  | { phase: "working" }
  | { phase: "sent" }
  | { phase: "failed"; message: string };

/** What the card's Test found last. */
export type CardTest =
  | { phase: "idle" }
  | { phase: "testing" }
  | { phase: "passed"; tools: number }
  | { phase: "failed"; message: string };

/**
 * Where a client stands with Writ: on the waiting list, decided on, added
 * from its card and not heard from yet, or none of these. Read off the same
 * lists the rest of the section shows, by the name the program sends.
 */
export type CardStanding =
  | { kind: "waiting"; client: PendingClient }
  | { kind: "decided"; approval: ClientApproval }
  | { kind: "expecting" }
  | { kind: "new" };

// Singleton state — Writ is single-window
const [cards, setCards] = createSignal<McpClientCard[] | null>(null);
const [loadFailed, setLoadFailed] = createSignal(false);
const [actions, setActions] = createSignal<Partial<Record<McpClientId, CardAction>>>({});
const [tests, setTests] = createSignal<Partial<Record<McpClientId, CardTest>>>({});

const ACTION_FAILED = "Writ could not be added.";
const TEST_FAILED = "The test could not run.";

/** Reads the cards: what each client is handed, and whether its command was
 * found. Read again each time the section opens, since a client installed
 * meanwhile is then found. */
async function load(): Promise<void> {
  try {
    setCards(await mcpConnectCards());
    setLoadFailed(false);
  } catch {
    setLoadFailed(true);
    logFailure("the client cards could not be read");
  }
}

function actionOf(id: McpClientId): CardAction {
  return actions()[id] ?? { phase: "idle" };
}

function testOf(id: McpClientId): CardTest {
  return tests()[id] ?? { phase: "idle" };
}

function setAction(id: McpClientId, action: CardAction): void {
  setActions((previous) => ({ ...previous, [id]: action }));
}

function setTest(id: McpClientId, test: CardTest): void {
  setTests((previous) => ({ ...previous, [id]: test }));
}

/**
 * Adds Writ to the client the card is for: runs its own `mcp add`, opens its
 * install link, or opens the Claude Desktop bundle. A client whose command was
 * not found has nothing to run, so its card copies the command instead.
 */
async function add(card: McpClientCard): Promise<void> {
  if (card.connection === "json" || (card.connection === "command" && !card.runs_command)) {
    await copy(card);
    return;
  }
  setAction(card.client, { phase: "working" });
  try {
    if (card.connection === "command") await mcpConnectAdd(card.client);
    else await mcpConnectOpen(card.client);
    setAction(card.client, { phase: "sent" });
  } catch (error) {
    setAction(card.client, { phase: "failed", message: userErrorText(error) ?? ACTION_FAILED });
  }
}

/** Puts the card's command or JSON on the clipboard. Throws when the
 * clipboard refuses it, so the caller can say so. */
async function copy(card: McpClientCard): Promise<void> {
  if (!card.copy_text) return;
  await writeClipboardText(card.copy_text);
  setAction(card.client, { phase: "sent" });
}

/** Starts the command every client is handed, as a client would, and counts
 * the tools it lists. */
async function test(id: McpClientId): Promise<void> {
  setTest(id, { phase: "testing" });
  try {
    const answer = await mcpServerTest();
    setTest(id, { phase: "passed", tools: answer.tools });
  } catch (error) {
    setTest(id, { phase: "failed", message: userErrorText(error) ?? TEST_FAILED });
  }
}

function standing(id: McpClientId): CardStanding {
  const waiting = activityStore.pending().find((client) => isClientsName(id, client.name));
  if (waiting) return { kind: "waiting", client: waiting };
  const approval = activityStore.clients().find((client) => isClientsName(id, client.name));
  if (approval) return { kind: "decided", approval };
  if (actionOf(id).phase === "sent") return { kind: "expecting" };
  return { kind: "new" };
}

export const mcpConnectStore = {
  cards,
  loadFailed,
  load,
  actionOf,
  testOf,
  add,
  copy,
  test,
  standing,
};
