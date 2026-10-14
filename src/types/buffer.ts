export type LineEnding = "lf" | "crlf";

export interface BufferDocument {
  id: string;
  title: string;
  filename: string;
  status: "active" | "history";
  language: string | null;
  source_path: string | null;
  // Where the tab was left, as CodeMirror positions (UTF-16 code units, line
  // breaks counted as one): the caret, and the start of the top visible line.
  cursor_pos: number;
  scroll_pos: number;
  tab_order: number;
  created_at: string;
  updated_at: string;
  closed_at: string | null;
  read_only: boolean;
  size_bytes: number;
  // The line ending the file on disk uses. The editor works in LF whatever the
  // file holds; the backend re-applies this before it writes.
  line_ending: LineEnding;
  // The text encoding the file is in, as Rust names it: the WHATWG name,
  // followed by " BOM" when the file starts with a byte order mark ("UTF-8",
  // "UTF-16LE BOM", "windows-1252"). The editor works in UTF-8; the backend
  // decodes on read and encodes on save (ADR-049). Null for a binary file's
  // hex view and a document Writ generated.
  encoding: string | null;
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
  | { kind: "Refused"; reason: string }
  // The file is a sync placeholder: its bytes are not on this machine. Nothing
  // was read and no buffer exists. The download is asked for separately and
  // the note is opened again once the bytes arrive.
  | { kind: "NotDownloaded"; path: string; provider: string | null };

export interface FileOpenResult {
  // Null when nothing opened: LargeFileConfirm, Refused and NotDownloaded.
  doc: BufferDocument | null;
  mode: FileOpenMode;
  size_bytes: number;
}

/** What Today's File answers: the dated file's path, and what opening it came to. */
export interface TodaysFile {
  path: string;
  opened: FileOpenResult;
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
