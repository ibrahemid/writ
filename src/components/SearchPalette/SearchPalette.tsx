import { createSignal } from "solid-js";
import Palette from "../Palette/Palette";
import { createCommandProvider } from "../../commands/providers/command-provider";
import { createSettingsProvider } from "../../commands/providers/settings-provider";
import { createFilesProvider } from "../../commands/providers/files-provider";
import { createContentProvider } from "../../commands/providers/content-provider";
import { createGotoLineProvider } from "../../commands/providers/goto-line-provider";
import { workspaceSearchStore } from "../../stores/global/workspace-search";
import type { ResultProvider } from "../Palette/types";

// Singleton state — Writ is single-window
const [isOpen, setIsOpen] = createSignal(false);

// Singleton state — Writ is single-window. Cleared on every close and on the
// keyboard path, so a query seeded from the editor's "Search workspace for …"
// row can never reappear the next time the palette is opened by shortcut.
const [seedQuery, setSeedQuery] = createSignal("");

export function openSearchPalette(query = "") {
  setSeedQuery(query);
  setIsOpen(true);
}
export function closeSearchPalette() {
  setIsOpen(false);
  setSeedQuery("");
}
export function toggleSearchPalette() {
  if (isOpen()) {
    closeSearchPalette();
    return;
  }
  openSearchPalette();
}

const PROVIDERS: ResultProvider[] = [
  createGotoLineProvider({ order: -1 }),
  createCommandProvider({
    excludeIds: ["search.openEverywhere"],
    order: 0,
    cap: 6,
    resultsLabel: "Commands",
  }),
  createSettingsProvider({ order: 1, cap: 4 }),
  createFilesProvider({ order: 2, cap: 8 }),
  createContentProvider({ order: 3, cap: 12 }),
];

const PREFIX_HINT = "> commands · # text · : line";

// Caps are stated, never silent (ADR-026).
export function searchNotice(): string | null {
  const parts: string[] = [];
  const status = workspaceSearchStore.indexStatus();
  if (status.truncated) {
    parts.push(`File index capped at ${status.file_count.toLocaleString()} files`);
  }
  const outcome = workspaceSearchStore.lastOutcome();
  if (outcome?.truncated) {
    parts.push(`Text search stopped at ${outcome.hit_count} matches`);
  }
  if (parts.length === 0) return PREFIX_HINT;
  return parts.join(" · ");
}

export default function SearchPalette() {
  return (
    <Palette
      open={isOpen()}
      onClose={closeSearchPalette}
      providers={PROVIDERS}
      placeholder="Type a command or note name"
      label="Search everywhere"
      inputLabel="Search files, text, commands"
      notice={searchNotice}
      onOpen={() => void workspaceSearchStore.refreshIndexStatus()}
      initialQuery={seedQuery}
    />
  );
}
