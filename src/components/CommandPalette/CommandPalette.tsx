import { createSignal } from "solid-js";
import Palette from "../Palette/Palette";
import { createCommandProvider } from "../../commands/providers/command-provider";
import { createSettingsProvider } from "../../commands/providers/settings-provider";
import type { ResultProvider } from "../Palette/types";

// Singleton state — Writ is single-window
const [isOpen, setIsOpen] = createSignal(false);

export function openCommandPalette() { setIsOpen(true); }
export function closeCommandPalette() { setIsOpen(false); }
export function toggleCommandPalette() { setIsOpen(prev => !prev); }

const PROVIDERS: ResultProvider[] = [
  createCommandProvider({
    excludeIds: ["palette.open"],
    listOnEmptyQuery: true,
    order: 0,
  }),
  createSettingsProvider({ order: 1 }),
];

export default function CommandPalette() {
  return (
    <Palette
      open={isOpen()}
      onClose={() => setIsOpen(false)}
      providers={PROVIDERS}
      placeholder="Search commands"
      label="Command palette"
      inputLabel="Command search"
    />
  );
}
