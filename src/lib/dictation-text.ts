const WORD_CHAR = /[\p{L}\p{N}\p{M}]/u;
const CLOSING = new Set([".", ",", ";", ":", "!", "?", "،", "؛", "؟", ")", "]", "}", '"', "'", "”", "’", "»", "…"]);
const NO_SPACE_BEFORE = new Set([",", ".", ";", ":", "!", "?", "،", "؛", "؟", ")"]);

function lastChar(text: string): string {
  const chars = Array.from(text);
  return chars[chars.length - 1] ?? "";
}

function firstChar(text: string): string {
  return Array.from(text)[0] ?? "";
}

/**
 * The text a dictated segment inserts between `before` and `after`, the
 * document text on either side of the insertion point: `text` trimmed, a space
 * in front when it follows a word or closing punctuation and does not open
 * with punctuation, and a space behind when a word follows. An empty `before`
 * is the start of the document. Answers "" for a segment of whitespace only.
 */
export function joinDictation(before: string, text: string, after: string): string {
  const trimmed = text.trim();
  if (trimmed === "") return "";
  const previous = lastChar(before);
  const opening = firstChar(trimmed);
  const spaceBefore =
    previous !== "" &&
    (WORD_CHAR.test(previous) || CLOSING.has(previous)) &&
    !NO_SPACE_BEFORE.has(opening);
  const spaceAfter = WORD_CHAR.test(firstChar(after));
  return `${spaceBefore ? " " : ""}${trimmed}${spaceAfter ? " " : ""}`;
}
