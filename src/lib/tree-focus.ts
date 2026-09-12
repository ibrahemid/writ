/**
 * Moves focus one visible row up or down a `role="tree"`, the arrow-key step
 * the folder tree and the tag tree share. A row the tree does not hold, or a
 * step past either end, leaves focus where it is.
 */
export function moveTreeFocus(tree: HTMLElement, from: HTMLElement, delta: 1 | -1): void {
  const items = Array.from(tree.querySelectorAll<HTMLElement>('[role="treeitem"]'));
  const index = items.indexOf(from);
  if (index === -1) return;
  items[index + delta]?.focus();
}
