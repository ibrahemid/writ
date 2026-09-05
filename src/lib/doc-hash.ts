const HEX_DIGITS = "0123456789abcdef";

/**
 * The text with every line break as a single `\n`.
 *
 * CodeMirror holds a document that way whatever the file used, so this is the
 * form both sides of the comparison are in. Without it a CRLF file would read
 * as edited from the moment it opened.
 *
 * The rule is `writ_core::hash::normalise_line_endings`; this mirrors it, and
 * `crates/writ-core/tests/fixtures/line-ending-digests.json` is read by a test
 * on each side to hold the two to the same answer.
 */
export function normaliseLineEndings(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}

/**
 * SHA-256 of `text` as 64 lowercase hex characters.
 *
 * The same digest `writ_core::hash::comparison_digest_hex` produces for the
 * file behind the note: the text is encoded as UTF-8, which is what a save
 * writes, and both sides normalise the line endings first.
 */
export async function hashDocument(text: string): Promise<string> {
  const bytes = new TextEncoder().encode(normaliseLineEndings(text));
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return toHex(new Uint8Array(digest));
}

function toHex(bytes: Uint8Array): string {
  let out = "";
  for (const byte of bytes) {
    out += HEX_DIGITS[byte >> 4] + HEX_DIGITS[byte & 0x0f];
  }
  return out;
}
