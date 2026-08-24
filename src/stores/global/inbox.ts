// Singleton state — Writ is single-window.
import { createSignal } from "solid-js";
import * as tauri from "../../services/tauri";
import { configStore } from "./config";
import { windowRegistry } from "./window-registry";
import { showToast } from "../../components/Notifications/Toast";
import { basename } from "../../lib/path";
import { logFailure } from "../../lib/log";

// Burst cap (ADR-018): at most BURST_CAP auto-opens per BURST_WINDOW_MS;
// further arrivals collapse into a single "N new files in inbox" toast.
const BURST_CAP = 3;
const BURST_WINDOW_MS = 2000;
const OVERFLOW_TOAST_DEBOUNCE_MS = 600;

const [path, setPath] = createSignal<string | null>(null);
const [files, setFiles] = createSignal<tauri.InboxFile[]>([]);

let openTimestamps: number[] = [];
let overflowCount = 0;
let overflowTimer: ReturnType<typeof setTimeout> | null = null;

async function refreshFiles(): Promise<void> {
  if (path() === null) {
    setFiles([]);
    return;
  }
  try {
    setFiles(await tauri.listInboxFiles());
  } catch {
    setFiles([]);
  }
}

async function hydrate(): Promise<void> {
  const current = await tauri.getInboxPath();
  setPath(current);
  await refreshFiles();
}

async function watchFolder(): Promise<string | null> {
  const picked = await tauri.pickInboxFolder();
  if (picked !== null) {
    setPath(picked);
    await refreshFiles();
  }
  return picked;
}

async function stopWatching(): Promise<void> {
  await tauri.clearInbox();
  setPath(null);
  setFiles([]);
}

async function handleFileArrived(filePath: string, nowMs: number = Date.now()): Promise<void> {
  openTimestamps = openTimestamps.filter((t) => nowMs - t < BURST_WINDOW_MS);
  if (openTimestamps.length >= BURST_CAP) {
    overflowCount += 1;
    scheduleOverflowToast();
    return;
  }
  openTimestamps.push(nowMs);

  const win = windowRegistry.getActive();
  if (!win) {
    logFailure("an inbox file arrived with no window to open into");
    showToast(`Couldn't open ${basename(filePath)} from the inbox`, "error");
    return;
  }
  try {
    await win.tabs.openFile(filePath);
  } catch {
    logFailure("an inbox file could not be opened");
    showToast(`Couldn't open ${basename(filePath)} from the inbox`, "error");
    return;
  }
  if (configStore.config().inbox.focus) {
    await tauri.showAndFocusWindow();
  }
  await refreshFiles();
}

function scheduleOverflowToast(): void {
  if (overflowTimer) clearTimeout(overflowTimer);
  overflowTimer = setTimeout(() => {
    overflowTimer = null;
    const count = overflowCount;
    overflowCount = 0;
    if (count > 0) {
      showToast(`${count} new file${count === 1 ? "" : "s"} in inbox`, "info");
    }
  }, OVERFLOW_TOAST_DEBOUNCE_MS);
}

export const inboxStore = {
  path,
  files,
  refreshFiles,
  hydrate,
  watchFolder,
  stopWatching,
  handleFileArrived,
};
