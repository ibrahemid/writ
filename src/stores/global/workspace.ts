// Singleton state — Writ is single-window.
import { createSignal } from "solid-js";
import * as tauri from "../../services/tauri";
import { dirname } from "../../lib/path";
import { createFolderListing } from "./folder-listing";

const [root, setRoot] = createSignal<string | null>(null);
const listing = createFolderListing((path) => tauri.listWorkspaceDir(path), root);

async function hydrate(): Promise<void> {
  const current = await tauri.getWorkspaceRoot();
  setRoot(current);
}

async function openFolder(): Promise<string | null> {
  const picked = await tauri.pickWorkspaceFolder();
  if (picked !== null) {
    listing.forgetAll();
    setRoot(picked);
  }
  return picked;
}

async function closeFolder(): Promise<void> {
  await tauri.clearWorkspaceRoot();
  listing.forgetAll();
  setRoot(null);
}

function handleChanged(path: string, removed: boolean): void {
  const currentRoot = root();
  if (!currentRoot) return;

  if (removed && listing.asked(path)) listing.forgetUnder(path);

  const parent = path === currentRoot ? currentRoot : dirname(path);
  if (listing.asked(parent)) void listing.loadDir(parent);
}

// More changed in the folder than the watcher listed: every folder the tree
// has asked for is read again, once.
function handleSwept(): void {
  if (!root()) return;
  for (const dirPath of listing.askedPaths()) void listing.loadDir(dirPath);
}

async function revealEntry(path: string): Promise<void> {
  await tauri.showWorkspaceEntryInFileManager(path);
}

export const workspaceStore = {
  root,
  hydrate,
  openFolder,
  closeFolder,
  loadDir: listing.loadDir,
  entriesFor: listing.entriesFor,
  errorFor: listing.errorFor,
  unreadableFor: listing.unreadableFor,
  isLoading: listing.isLoading,
  revealEntry,
  handleChanged,
  handleSwept,
};
