import { placeholder } from "@codemirror/view";
import type { Extension } from "@codemirror/state";

export type PlaceholderKind = "markdown" | "plain" | "none";

export const MARKDOWN_PLACEHOLDER = "Type # for a heading, [[ to link a file";

export function placeholderText(kind: PlaceholderKind, paletteChord: string): string | null {
  if (kind === "markdown") return MARKDOWN_PLACEHOLDER;
  if (kind === "plain" && paletteChord !== "") return `Press ${paletteChord} for the command palette`;
  return null;
}

export function placeholderExtension(kind: PlaceholderKind, paletteChord: string): Extension {
  const text = placeholderText(kind, paletteChord);
  return text === null ? [] : placeholder(text);
}
