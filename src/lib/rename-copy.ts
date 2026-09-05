/**
 * The two sentences a propagated rename says.
 *
 * Here rather than in the components that show them, so the question and the
 * report cannot drift apart and so both can be read in a test without
 * rendering anything.
 */

/** What the rename asks before it runs, when other notes link to this one. */
export function linkCountQuestion(count: number): string {
  return count === 1
    ? "1 note links here. Update it?"
    : `${count} notes link here. Update them?`;
}

/** What the rename says afterwards about the notes it could not rewrite. */
export function unchangedHeading(count: number): string {
  return count === 1 ? "Left 1 note unchanged:" : `Left ${count} notes unchanged:`;
}
