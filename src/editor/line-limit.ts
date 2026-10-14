import type { EditorView } from "@codemirror/view";
import { MAX_NORMAL_LINE_LENGTH } from "./large-file";

/** A stretch of the document, as `EditorView.visibleRanges` gives one. */
export interface DocRange {
  from: number;
  to: number;
}

/**
 * The view's visible ranges with every line longer than `limit` taken out.
 *
 * The per-line decorations (Markdown rendering, links, wikilinks) read whole
 * lines. A line past the limit is drawn plain and the rest of the file keeps
 * them, so one pasted blob does not turn them off for the whole note.
 */
export function rangesWithinLineLimit(view: EditorView, limit = MAX_NORMAL_LINE_LENGTH): DocRange[] {
  const { doc } = view.state;
  const out: DocRange[] = [];
  for (const { from, to } of view.visibleRanges) {
    let start = from;
    for (let pos = from; pos <= to; ) {
      const line = doc.lineAt(pos);
      if (line.length > limit) {
        if (line.from > start) out.push({ from: start, to: line.from - 1 });
        start = Math.min(line.to + 1, to);
      }
      if (line.to >= to) break;
      pos = line.to + 1;
    }
    if (start < to) out.push({ from: start, to });
  }
  return out;
}
