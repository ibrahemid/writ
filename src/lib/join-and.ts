/** `a`, `a and b`, `a, b and c` — the app's own list voice for a sentence. */
export function joinAnd(parts: readonly string[]): string {
  if (parts.length < 2) return parts[0] ?? "";
  return `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
}
