export interface BufferDocument {
  id: string;
  title: string;
  filename: string;
  status: "active" | "history";
  language: string | null;
  source_path: string | null;
  cursor_pos: number;
  scroll_pos: number;
  tab_order: number;
  created_at: string;
  updated_at: string;
  closed_at: string | null;
  read_only: boolean;
  size_bytes: number;
}

export type FileOpenMode =
  | { kind: "Normal" }
  | { kind: "LargeFile" }
  | { kind: "LargeFileConfirm" }
  // Frontend-only: a sub-threshold file whose line shape (a single very long
  // line, as in minified JS/JSON/CSS) would freeze the editor under the full
  // extension set. The backend never emits this; it is derived from content.
  | { kind: "LongLines" }
  | { kind: "Binary" }
  | { kind: "Refused"; reason: string };

export interface FileOpenResult {
  doc: BufferDocument;
  mode: FileOpenMode;
  size_bytes: number;
}

/**
 * What the person chose about a file that changed outside Writ.
 *
 * The words are the wire form of `writ_core::notes::reload::ChangeChoice`.
 * Every one of them writes the text it does not keep to its own file first,
 * so no answer ends with a text that exists nowhere.
 */
export type ChangeChoice = "keep_mine" | "use_disk" | "keep_both";

/** What resolving a change outside Writ left behind. */
export interface ResolveOutcome {
  /**
   * The file the text that was not kept was written to. Null only when the
   * two texts turned out to be the same text.
   */
  conflict_copy_path: string | null;
  /** What the tab must show now, or null when it keeps what it holds. */
  content: string | null;
  /** The digest of what the note's file holds once the choice has run. */
  disk_hash: string;
}
