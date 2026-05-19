import type { BufferDocument } from "../../types/buffer";

export interface MatchedBuffers {
  active: BufferDocument[];
  history: BufferDocument[];
}

export function matchedBuffers(
  query: string,
  matchedIds: readonly string[],
  active: readonly BufferDocument[],
  history: readonly BufferDocument[],
): MatchedBuffers {
  const q = query.toLowerCase().trim();
  if (!q) return { active: [], history: [] };
  const idSet = new Set(matchedIds);
  const hit = (b: BufferDocument) =>
    b.title.toLowerCase().includes(q) || idSet.has(b.id);
  return {
    active: active.filter(hit),
    history: history.filter(hit),
  };
}
