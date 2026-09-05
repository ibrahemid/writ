export type LineEnding = "lf" | "crlf";

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
  // The line ending the file on disk uses. The editor works in LF whatever the
  // file holds; the backend re-applies this before it writes.
  line_ending: LineEnding;
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
  // Null for NotDownloaded, the one mode that opens no buffer.
  doc: BufferDocument | null;
  mode: FileOpenMode;
  size_bytes: number;
}
