import type { EditorState } from "@codemirror/state";
import type { SearchQuery } from "@codemirror/search";

export const MATCH_COUNT_CAP = 10000;

export interface CountResult {
  current: number;
  total: number;
  capped: boolean;
}

const EMPTY: CountResult = { current: 0, total: 0, capped: false };

export function countMatches(
  state: EditorState,
  query: SearchQuery,
  cap: number = MATCH_COUNT_CAP,
): CountResult {
  if (!query.search || !query.valid) return EMPTY;

  const sel = state.selection.main;
  const cursor = query.getCursor(state);
  let total = 0;
  let current = 0;

  let res = cursor.next();
  while (!res.done) {
    const { from, to } = res.value;
    total += 1;
    if (from === sel.from && to === sel.to) current = total;
    if (total >= cap) return { current, total, capped: true };
    res = cursor.next();
  }

  return { current, total, capped: false };
}
