// Words are runs separated by whitespace, which is what a writer counts. CJK
// text has no such separators, so a run of ideographs counts as one word: the
// number is a writing-progress cue, not a linguistic measure.
const WHITESPACE = /\s+/;

export function countWords(text: string): number {
  const trimmed = text.trim();
  if (trimmed === "") return 0;
  return trimmed.split(WHITESPACE).length;
}

export function formatWordCount(count: number): string {
  return `${count.toLocaleString()} ${count === 1 ? "word" : "words"}`;
}
