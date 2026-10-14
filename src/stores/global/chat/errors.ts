/** What a refused call says, in the words it was refused in where it had
 * any. */
export function readableError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "The reply did not arrive.";
}
