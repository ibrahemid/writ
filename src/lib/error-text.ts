// Rust writes some refusals for people ("Could not open the link.") and leaves
// the rest as the Display text of its error enums, which is for logs:
// lowercase type words ("buffer not found: 3f2a…"), raw OS text or an ERR_ code.

const ERROR_CODE = /^ERR_[A-Z_]+/;
const SENTENCE_START = /^[A-Z“"‘']/;
const SENTENCE_END = /[.?!…]["”’')]?$/;
const LOG_WORDS = /\b(?:buffers?|MiB|KiB|GiB|io error|errno|os error)\b/i;

function rawText(error: unknown): string {
  if (typeof error === "string") return error.trim();
  if (error instanceof Error) return error.message.trim();
  return "";
}

export function userErrorText(error: unknown): string | null {
  const text = rawText(error);
  if (text === "") return null;
  if (ERROR_CODE.test(text)) return null;
  if (!SENTENCE_START.test(text) || !SENTENCE_END.test(text)) return null;
  if (LOG_WORDS.test(text) || text.includes("—")) return null;
  return text;
}
