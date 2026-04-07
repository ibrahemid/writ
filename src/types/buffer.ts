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
}
