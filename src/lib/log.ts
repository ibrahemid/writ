/**
 * The one console line Writ writes, for a failure the user has already been
 * told about or one that silently drops a guarantee.
 *
 * The message must be short, stable and safe to read over someone's shoulder:
 * no file paths, no buffer or query text, no tokens, and never a raw error
 * object. The detail belongs in the message the user sees, not here.
 */
export function logFailure(message: string): void {
  console.error(`[writ] ${message}`);
}
