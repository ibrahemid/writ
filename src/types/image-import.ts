/** What the host did with a paste into a Markdown file. */
export type ClipboardPasteResult =
  | { kind: "inserted"; markdown: string; notices: string[] }
  | { kind: "refused"; notices: string[] }
  | { kind: "text" }
  | { kind: "empty" };

/** What the host did with files dropped on a Markdown file. */
export interface DroppedImagesResult {
  /** Image references to insert, one per line; empty when none was taken. */
  markdown: string;
  /** Dropped paths that are not images, left to open as tabs. */
  others: string[];
  /** One message per image not taken. */
  notices: string[];
}

/** A drop position in CSS pixels from the window's top-left corner. */
export interface DropPoint {
  x: number;
  y: number;
}
