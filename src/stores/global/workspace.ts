// Singleton state — Writ is single-window.
import { createSignal } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import * as tauri from "../../services/tauri";
import { dirname } from "../../lib/path";
import type { WorkspaceEntry } from "../../types/workspace";

const [root, setRoot] = createSignal<string | null>(null);
const [dirs, setDirs] = createStore<Record<string, WorkspaceEntry[] | undefined>>({});

async function hydrate(): Promise<void> {
  const current = await tauri.getWorkspaceRoot();
  setRoot(current);
}

async function openFolder(): Promise<string | null> {
  const picked = await tauri.pickWorkspaceFolder();
  if (picked !== null) {
    setDirs(reconcile({}));
    setRoot(picked);
  }
  return picked;
}

async function closeFolder(): Promise<void> {
  await tauri.clearWorkspaceRoot();
  setDirs(reconcile({}));
  setRoot(null);
}

async function loadDir(dirPath: string): Promise<void> {
  try {
    const entries = await tauri.listWorkspaceDir(dirPath);
    setDirs(dirPath, entries);
  } catch {
    setDirs(dirPath, undefined);
  }
}

function entriesFor(dirPath: string): WorkspaceEntry[] | undefined {
  return dirs[dirPath];
}

function handleChanged(path: string, removed: boolean): void {
  const currentRoot = root();
  if (!currentRoot) return;

  if (removed && dirs[path] !== undefined) {
    setDirs(path, undefined);
  }

  const parent = path === currentRoot ? currentRoot : dirname(path);
  if (dirs[parent] !== undefined) {
    void loadDir(parent);
  }
}

export const workspaceStore = {
  root,
  hydrate,
  openFolder,
  closeFolder,
  loadDir,
  entriesFor,
  handleChanged,
};
