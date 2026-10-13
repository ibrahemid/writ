export function formatWordCount(count: number): string {
  return `${count.toLocaleString()} ${count === 1 ? "word" : "words"}`;
}
