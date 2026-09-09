import { createSignal } from "solid-js";

import { onEvent, type UnlistenFn } from "../../services/events";
import {
  activityClear,
  activityRecent,
  mcpClients,
  mcpForgetClient,
  mcpServerCommand,
  mcpSetClientPermission,
  type ActivityRecord,
  type ClientApproval,
  type McpClients,
  type McpServerCommand,
  type PendingClient,
} from "../../services/tauri";
import { writeClipboardText } from "../../services/clipboard";
import { logFailure } from "../../lib/log";

export type { ActivityRecord, ClientApproval, McpServerCommand, PendingClient };

// Singleton state — Writ is single-window

/** How many records the panel holds. The list is read, not exported. */
export const ACTIVITY_LIMIT = 200;

/** How often the panel re-reads while it is on screen, in milliseconds. */
export const ACTIVITY_POLL_MS = 5_000;

const [records, setRecords] = createSignal<ActivityRecord[]>([]);
const [clients, setClients] = createSignal<ClientApproval[]>([]);
const [pending, setPending] = createSignal<PendingClient[]>([]);
const [serverCommand, setServerCommand] = createSignal<McpServerCommand | null>(null);

let subscription: Promise<UnlistenFn> | null = null;

function takeClients(answer: McpClients): void {
  setClients(answer.approved);
  setPending(answer.waiting);
}

async function refresh(): Promise<void> {
  try {
    setRecords(await activityRecent(ACTIVITY_LIMIT));
  } catch {
    logFailure("the activity list could not be read");
  }
}

async function refreshClients(): Promise<void> {
  try {
    takeClients(await mcpClients());
  } catch {
    logFailure("the list of connected programs could not be read");
  }
}

/**
 * One listener for the app's own changes to the log, opened the first time the
 * panel loads rather than at launch. The server process cannot emit, so this is
 * not every change and the panel polls while it is on screen.
 */
function subscribe(): Promise<UnlistenFn> {
  subscription ??= onEvent("activity:changed", () => {
    void refresh();
  });
  return subscription;
}

/** Reads the log, the programs and the command, and starts listening. */
async function load(): Promise<void> {
  void subscribe();
  await Promise.all([refresh(), refreshClients(), loadCommand()]);
}

/** Reads the command a client is given, once per session. */
async function loadCommand(): Promise<void> {
  if (serverCommand()) return;
  try {
    setServerCommand(await mcpServerCommand());
  } catch {
    logFailure("the server command could not be read");
  }
}

/** Puts the command on the clipboard, for pasting into a client. */
async function copyCommand(): Promise<void> {
  const command = serverCommand()?.command;
  if (!command) return;
  await writeClipboardText(command);
}

/** Forgets both generations of the log. */
async function clear(): Promise<void> {
  await activityClear();
  setRecords([]);
}

/**
 * Grants or revokes one program's directions, by the name it sent.
 *
 * Writing carries reading with it: every write tool reads the note before it
 * replaces it, so a grant of one without the other would refuse calls the user
 * had just approved.
 */
async function setPermission(name: string, read: boolean, write: boolean): Promise<void> {
  takeClients(await mcpSetClientPermission(name, read || write, write));
  await refresh();
}

/** Takes a program off the list, so its next call waits to be decided on. */
async function forget(name: string): Promise<void> {
  takeClients(await mcpForgetClient(name));
  await refresh();
}

export const activityStore = {
  records,
  clients,
  pending,
  serverCommand,
  load,
  refresh,
  refreshClients,
  loadCommand,
  copyCommand,
  clear,
  setPermission,
  forget,
};
