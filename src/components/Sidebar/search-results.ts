import type { BufferDocument } from "../../types/buffer";
import type { SearchHit, SnippetSegment } from "../../types/search";

export type SearchRowSource = "active" | "history" | "file" | "other";

export interface SearchRow {
  id: string;
  title: string;
  line: number | null;
  segments: SnippetSegment[];
  source: SearchRowSource;
  // Set on a row the index found on disk that no tab is showing. Opening it
  // goes through the path, because there is no id to focus.
  path: string | null;
}

// Builds the sidebar's flat result list. Index hits come first (in rank order,
// with snippet + line), followed by any open or history buffer whose *title*
// matches but is absent from the index — large files and binaries are never
// indexed, so this title pass keeps them findable. Title-only rows carry no
// line and a literally-highlighted title as their snippet.
//
// A hit the index found on disk that no tab is showing arrives with an empty
// buffer id and a path (ADR-028 section 7); it is keyed and opened by that
// path.
export function buildSearchRows(
  hits: readonly SearchHit[],
  query: string,
  active: readonly BufferDocument[],
  history: readonly BufferDocument[],
): SearchRow[] {
  const q = query.toLowerCase().trim();
  const sourceById = new Map<string, SearchRowSource>();
  for (const b of active) sourceById.set(b.id, "active");
  for (const b of history) if (!sourceById.has(b.id)) sourceById.set(b.id, "history");

  const rows: SearchRow[] = hits.map((hit) => {
    const known = hit.buffer_id ? sourceById.get(hit.buffer_id) : undefined;
    const path = hit.path ?? null;
    return {
      id: hit.buffer_id || path || hit.title,
      title: hit.title,
      line: hit.line,
      segments: hit.snippet,
      source: known ?? (path ? "file" : "other"),
      path,
    };
  });

  if (!q) return rows;

  const seen = new Set(hits.map((h) => h.buffer_id).filter(Boolean));
  for (const b of [...active, ...history]) {
    if (seen.has(b.id) || !b.title.toLowerCase().includes(q)) continue;
    seen.add(b.id);
    rows.push({
      id: b.id,
      title: b.title,
      line: null,
      segments: highlightLiteral(b.title, query),
      source: sourceById.get(b.id) ?? "other",
      path: b.source_path,
    });
  }

  return rows;
}

// Splits `text` into matched / unmatched runs around case-insensitive
// occurrences of the trimmed `query` string. Used for title-only rows the
// backend never scored; FTS hits arrive pre-segmented from Rust.
export function highlightLiteral(text: string, query: string): SnippetSegment[] {
  const needle = query.toLowerCase().trim();
  if (!needle) return [{ text, matched: false }];

  const lower = text.toLowerCase();
  const segments: SnippetSegment[] = [];
  let pos = 0;
  let idx = lower.indexOf(needle, pos);
  while (idx !== -1) {
    if (idx > pos) segments.push({ text: text.slice(pos, idx), matched: false });
    segments.push({ text: text.slice(idx, idx + needle.length), matched: true });
    pos = idx + needle.length;
    idx = lower.indexOf(needle, pos);
  }
  if (pos < text.length) segments.push({ text: text.slice(pos), matched: false });
  return segments;
}
