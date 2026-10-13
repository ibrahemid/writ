import { createSignal } from "solid-js";
import Palette from "../Palette/Palette";
import { createLanguageProvider } from "../../commands/providers/language-provider";
import type { ResultProvider } from "../Palette/types";

// Singleton state — Writ is single-window
const [isOpen, setIsOpen] = createSignal(false);

/** Opens the language list for the file in front (Change Language). */
export function openLanguagePicker(): void {
  setIsOpen(true);
}

export function closeLanguagePicker(): void {
  setIsOpen(false);
}

const PROVIDERS: ResultProvider[] = [createLanguageProvider()];

export default function LanguagePicker() {
  return (
    <Palette
      open={isOpen()}
      onClose={closeLanguagePicker}
      providers={PROVIDERS}
      placeholder="Type a language name"
      label="Change language"
      inputLabel="Language name"
    />
  );
}
