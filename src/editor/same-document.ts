import type { Text } from "@codemirror/state";

/**
 * Whether two documents hold the same text, as `Text.eq` answers it.
 *
 * `Text.eq` skips the children the two trees share at the start and at the
 * end, then reads every line between the first difference and the last, so
 * an edit near the top and one near the bottom, both undone, read the whole
 * document between them. A child holds whole lines, so where the two trees
 * line up child by child (as many children, each pair holding as many lines)
 * every pair holds the same lines of both documents, and the documents are
 * equal exactly when every pair is: each pair is compared on its own, and a
 * child both trees hold is skipped. Where they do not line up, `Text.eq`
 * compares that subtree.
 */
export function sameDocument(a: Text, b: Text): boolean {
  if (a === b) return true;
  if (a.length !== b.length || a.lines !== b.lines) return false;
  const left = a.children;
  const right = b.children;
  if (left === null || right === null || left.length !== right.length) return a.eq(b);
  for (let i = 0; i < left.length; i++) {
    if (left[i].lines !== right[i].lines) return a.eq(b);
  }
  for (let i = 0; i < left.length; i++) {
    if (!sameDocument(left[i], right[i])) return false;
  }
  return true;
}
