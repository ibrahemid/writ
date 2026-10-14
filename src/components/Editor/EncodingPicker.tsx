import { createSignal } from "solid-js";
import Palette from "../Palette/Palette";
import { createEncodingProvider } from "../../commands/providers/encoding-provider";
import type { ResultProvider } from "../Palette/types";

// Singleton state — Writ is single-window
const [isOpen, setIsOpen] = createSignal(false);

/** Opens the encoding list for the file in front (Reopen with Encoding). */
export function openEncodingPicker(): void {
  setIsOpen(true);
}

export function closeEncodingPicker(): void {
  setIsOpen(false);
}

const PROVIDERS: ResultProvider[] = [createEncodingProvider()];

export default function EncodingPicker() {
  return (
    <Palette
      open={isOpen()}
      onClose={closeEncodingPicker}
      providers={PROVIDERS}
      placeholder="Type an encoding name"
      label="Reopen with encoding"
      inputLabel="Encoding name"
    />
  );
}
