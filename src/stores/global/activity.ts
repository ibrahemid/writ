import { createSignal } from "solid-js";

import { onEvent, type UnlistenFn } from "../../services/events";
import {
  activityClear,
  activityRecent,
  mcpClients,
  mcpDenyClient,
  mcpForgetClient,
  mcpRestoreClient,
  mcpReturnToWaiting,
  mcpServerCommand,
  mcpSetClientPermission,
  mcpTools,
  type ActivityRecord,
  type ClientApproval,
  type McpClients,
  type McpServerCommand,
  type McpTools,
  type PendingClient,
} from "../../services/tauri";
import { writeClipboardText } from "../../services/tauri";
import { logFailure } from "../../lib/log";
import { SECTION_LABELS, settingEntry, UnknownSettingError } from "../../settings";

export type { ActivityRecord, ClientApproval, McpServerCommand, McpTools, PendingClient };

// Singleton state — Writ is single-window

/** How many records the panel holds. The list is read, not exported. */
export const ACTIVITY_LIMIT = 200;

/** How often the panel re-reads while it is on screen, in milliseconds. */
export const ACTIVITY_POLL_MS = 5_000;

const [records, setRecords] = createSignal<ActivityRecord[]>([]);
// Whether the last read of the log failed, so the panel can say so rather than
// show an empty log as if no program had called.
const [logFailed, setLogFailed] = createSignal(false);
const [clients, setClients] = createSignal<ClientApproval[]>([]);
const [pending, setPending] = createSignal<PendingClient[]>([]);
const [serverCommand, setServerCommand] = createSignal<McpServerCommand | null>(null);
const [tools, setTools] = createSignal<McpTools | null>(null);
const [announced, setAnnounced] = createSignal<string[]>([]);

let subscription: Promise<UnlistenFn> | null = null;
let waitingSubscription: Promise<UnlistenFn> | null = null;

function isWaiting(waiting: readonly PendingClient[], name: string): boolean {
  return waiting.some((client) => client.name === name);
}

function takeClients(answer: McpClients): void {
  setClients(answer.approved);
  setPending(answer.waiting);
  setAnnounced((names) => names.filter((name) => isWaiting(answer.waiting, name)));
}

function takeWaiting(waiting: PendingClient[]): void {
  const before = pending();
  const arrived = waiting
    .map((client) => client.name)
    .filter((name) => !isWaiting(before, name));
  setPending(waiting);
  setAnnounced((names) => [
    ...names.filter((name) => isWaiting(waiting, name)),
    ...arrived.filter((name) => !names.includes(name)),
  ]);
}

/**
 * Everything the panel shows: the log and who is waiting to be decided on.
 *
 * One call for both, because both go stale the same way. The waiting list is
 * its own file rather than a reading of the log, so a caller that re-read only
 * the log left a program that first called while the panel was open invisible.
 */
async function refresh(): Promise<void> {
  await Promise.all([refreshLog(), refreshClients()]);
}

async function refreshLog(): Promise<void> {
  try {
    setRecords(await activityRecent(ACTIVITY_LIMIT));
    setLogFailed(false);
  } catch {
    setLogFailed(true);
    logFailure("the activity list could not be read");
  }
}

/** The programs alone, for the settings section, which shows no log. */
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

async function watchWaiting(): Promise<void> {
  waitingSubscription ??= onEvent("mcp:pending", (payload) => {
    takeWaiting(payload.waiting);
  });
  await waitingSubscription;
  await refreshClients();
}

function isDecided(name: string): boolean {
  return clients().some((client) => client.name === name);
}

function isNotAllowed(name: string): boolean {
  return clients().some((client) => client.name === name && !client.read && !client.write);
}

function notice(): PendingClient | null {
  const waiting = pending();
  for (const name of announced()) {
    const client = waiting.find((entry) => entry.name === name);
    if (client) return client;
  }
  return null;
}

function dismissNotice(name: string): void {
  setAnnounced((names) => names.filter((entry) => entry !== name));
}

function announce(name: string): void {
  if (!isWaiting(pending(), name)) return;
  setAnnounced((names) => [name, ...names.filter((entry) => entry !== name)]);
}

/** Reads the log, the programs and the command, and starts listening. */
async function load(): Promise<void> {
  void subscribe();
  await Promise.all([refresh(), loadCommand()]);
}

/**
 * Reads the command a client is given, once per session: again only while the
 * command still waits on the install, which the person can do meanwhile.
 */
async function loadCommand(): Promise<void> {
  const current = serverCommand();
  if (current && !current.needs_install) return;
  try {
    setServerCommand(await mcpServerCommand());
  } catch {
    logFailure("the server command could not be read");
  }
}

/** Reads the tool list the server registers, once per session. */
async function loadTools(): Promise<void> {
  if (tools()) return;
  try {
    setTools(await mcpTools());
  } catch {
    logFailure("the tool list could not be read");
  }
}

/**
 * What has to happen before the command is worth giving a program, which
 * keeps the path it was given: a translocated app's path changes at every
 * launch, and an AppImage's copy is only there once installed.
 */
function commandCaution(): string | undefined {
  const command = serverCommand();
  if (command?.needs_move) {
    return "Move Writ to your Applications folder and open it from there first.";
  }
  if (command?.needs_install) {
    return `Install the writ command first, under ${cliRowPath()}.`;
  }
  return undefined;
}

const CLI_SETTING_ID = "files.cli";

/** Where the install lives, read from the settings index. */
function cliRowPath(): string {
  const entry = settingEntry(CLI_SETTING_ID);
  if (!entry) throw new UnknownSettingError(CLI_SETTING_ID);
  return `Settings, ${SECTION_LABELS[entry.section]}, ${entry.title}`;
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
 * had just approved. A grant of writing waits on a confirmation outside the
 * window; turned down, the lists come back unchanged.
 */
async function setPermission(name: string, read: boolean, write: boolean): Promise<void> {
  takeClients(await mcpSetClientPermission(name, read || write, write));
  await refreshLog();
}

/**
 * "Don't allow": stores a decision granting neither direction, by the name the
 * program sent, so it is answered as not allowed and does not wait again.
 */
async function deny(name: string): Promise<void> {
  takeClients(await mcpDenyClient(name));
}

async function undoDenial(name: string): Promise<void> {
  takeClients(await mcpReturnToWaiting(name));
}

/** Takes a program off the list, so its next call waits to be decided on. */
async function forget(name: string): Promise<void> {
  takeClients(await mcpForgetClient(name));
  await refreshLog();
}

/**
 * Puts a forgotten program back as it was, and answers whether it is back. An
 * approval that carries writing waits on a confirmation outside the window,
 * which the person can turn down.
 */
async function restore(approval: ClientApproval): Promise<boolean> {
  takeClients(await mcpRestoreClient(approval));
  return clients().some((client) => client.name === approval.name);
}

export const activityStore = {
  records,
  logFailed,
  clients,
  pending,
  serverCommand,
  commandCaution,
  tools,
  load,
  refresh,
  refreshClients,
  watchWaiting,
  isDecided,
  isNotAllowed,
  notice,
  dismissNotice,
  announce,
  loadCommand,
  loadTools,
  copyCommand,
  clear,
  setPermission,
  deny,
  undoDenial,
  forget,
  restore,
};
