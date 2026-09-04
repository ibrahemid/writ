import { createSignal } from "solid-js";
import Palette from "../Palette/Palette";
import { createCommandProvider } from "../../commands/providers/command-provider";
import { createNotesProvider } from "../../commands/providers/notes-provider";
import { createSettingsProvider } from "../../commands/providers/settings-provider";
import type { ResultProvider } from "../Palette/types";

// Singleton state — Writ is single-window
const [isOpen, setIsOpen] = createSignal(false);
// What the input is seeded with on the next open. The name-search mode is a
// routing prefix, so opening "in" it is opening with that prefix already typed.
const [seed, setSeed] = createSignal("");

export function openCommandPalette() { setSeed(""); setIsOpen(true); }
export function closeCommandPalette() { setIsOpen(false); }
export function toggleCommandPalette() { setSeed(""); setIsOpen(prev => !prev); }

// Opens the palette listing notes by name, the surface behind `notes.quickOpen`.
export function openNoteSearch() { setSeed(NOTES_PREFIX); setIsOpen(true); }

const NOTES_PREFIX = "@";

const PROVIDERS: ResultProvider[] = [
  createCommandProvider({
    excludeIds: ["palette.open"],
    listOnEmptyQuery: true,
    order: 0,
  }),
  createNotesProvider({ order: 1 }),
  createSettingsProvider({ order: 2 }),
];

export default function CommandPalette() {
  return (
    <Palette
      open={isOpen()}
      onClose={() => setIsOpen(false)}
      providers={PROVIDERS}
      initialQuery={seed}
      placeholder="Search commands"
      label="Command palette"
      inputLabel="Command search"
    />
  );
}
