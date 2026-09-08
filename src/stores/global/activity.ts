import { createSignal, type Accessor } from "solid-js";

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
  type McpServerCommand,
} from "../../services/tauri";
import { writeClipboardText } from "../../services/clipboard";
import { logFailure } from "../../lib/log";

export type { ActivityRecord, ClientApproval, McpServerCommand };

// Singleton state — Writ is single-window

/** How many records the panel holds. The list is read, not exported. */
export const ACTIVITY_LIMIT = 200;

/** How often the panel re-reads while it is on screen, in milliseconds. */
export const ACTIVITY_POLL_MS = 5_000;

const [records, setRecords] = createSignal<ActivityRecord[]>([]);
const [clients, setClients] = createSignal<ClientApproval[]>([]);
const [serverCommand, setServerCommand] = createSignal<McpServerCommand | null>(null);

let subscription: Promise<UnlistenFn> | null = null;

/**
 * A program the log has seen that the user has not decided on yet.
 *
 * Derived from the records rather than stored: the `writ mcp` process writes
 * the pending line and has no way to tell a running app about it, so the log is
 * where a waiting program shows up. A name that is on the approved list is not
 * waiting, whichever way that decision went.
 */
export interface PendingClient {
  name: string;
  version: string | null;
  /** The newest call it made, as the record spells the time. */
  at: string;
}

function pendingFrom(rows: ActivityRecord[], decided: ClientApproval[]): PendingClient[] {
  const known = new Set(decided.map((client) => client.name));
  const waiting = new Map<string, PendingClient>();
  for (const row of rows) {
    if (row.decision !== "pending") continue;
    if (row.actor.kind !== "client") continue;
    if (known.has(row.actor.name) || waiting.has(row.actor.name)) continue;
    waiting.set(row.actor.name, {
      name: row.actor.name,
      version: row.actor.version,
      at: row.at,
    });
  }
  return [...waiting.values()];
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
    setClients(await mcpClients());
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

/** Reads the log, the approvals and the command, and starts listening. */
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

/** Grants or revokes one program's directions, by the name it sent. */
async function setPermission(name: string, read: boolean, write: boolean): Promise<void> {
  setClients(await mcpSetClientPermission(name, read, write));
  await refresh();
}

/** Takes a program off the list, so its next call waits to be decided on. */
async function forget(name: string): Promise<void> {
  setClients(await mcpForgetClient(name));
  await refresh();
}

const pending: Accessor<PendingClient[]> = () => pendingFrom(records(), clients());

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

/** Test seam: the derivation, without the signals around it. */
export const __testing = { pendingFrom };
