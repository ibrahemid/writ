/** A heading as the outline lists it: where it is, 1-based. */
export interface OutlineHeading {
  line: number;
}

/**
 * The heading the caret is under: the last one at or above `cursorLine`, by
 * index into `headings` (in the order they are written), or `-1` when the
 * caret is above the first heading or the note has none.
 */
export function currentHeadingIndex(
  headings: readonly OutlineHeading[],
  cursorLine: number,
): number {
  let current = -1;
  for (let i = 0; i < headings.length; i += 1) {
    if (headings[i].line > cursorLine) break;
    current = i;
  }
  return current;
}
