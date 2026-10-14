import type { PaletteMode } from "./types";

export interface ParsedPaletteQuery {
  mode: PaletteMode;
  // The query with its prefix stripped and trimmed.
  text: string;
  prefix: string;
}

/** The routing prefixes, in the order the palettes' footers list them. */
export const PALETTE_PREFIXES: readonly { prefix: string; mode: PaletteMode; label: string }[] = [
  { prefix: ">", mode: "commands", label: "Commands" },
  { prefix: "@", mode: "notes", label: "File names" },
  { prefix: "#", mode: "content", label: "Text" },
  { prefix: ":", mode: "line", label: "Go to line" },
];

const PREFIX_MODES: ReadonlyMap<string, PaletteMode> = new Map(
  PALETTE_PREFIXES.map((entry) => [entry.prefix, entry.mode]),
);

// Splits a raw palette query into its routing prefix and the remaining terms.
// An unprefixed query runs every provider; a prefixed one runs only the
// providers that declare that mode.
export function parsePaletteQuery(raw: string): ParsedPaletteQuery {
  const leading = raw.trimStart();
  const prefix = leading.slice(0, 1);
  const mode = PREFIX_MODES.get(prefix);
  if (!mode) return { mode: "all", text: raw.trim(), prefix: "" };
  return { mode, text: leading.slice(1).trim(), prefix };
}
