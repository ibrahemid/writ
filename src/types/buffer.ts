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
  | { kind: "Binary" }
  | { kind: "Refused"; reason: string };

export interface FileOpenResult {
  doc: BufferDocument;
  mode: FileOpenMode;
  size_bytes: number;
}
