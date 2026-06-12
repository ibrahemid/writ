const CHAR_RULE_WEIGHT: f64 = 0.6;
const WORD_RULE_WEIGHT: f64 = 0.4;
const CHARS_PER_TOKEN: f64 = 4.0;
const TOKENS_PER_WORD: f64 = 4.0 / 3.0;

/// Estimates the LLM token count of `text` using a blended heuristic.
///
/// Blends the ~4-characters-per-token and ~1.33-tokens-per-word rules of
/// thumb, weighted toward the character rule, which tracks mixed
/// prose/code buffers more reliably. For English prose, Markdown, and
/// mainstream code the result lands within roughly ±30% of cl100k-family
/// tokenizers; CJK and similar scripts undercount by design (ADR-015).
///
/// Whitespace-only input estimates to zero. Non-empty trimmed input always
/// estimates to at least one token.
pub fn estimate_tokens(text: &str) -> usize {
    if text.trim().is_empty() {
        return 0;
    }
    let chars = text.chars().count() as f64;
    let words = text.split_whitespace().count() as f64;
    let blended = CHAR_RULE_WEIGHT * (chars / CHARS_PER_TOKEN)
        + WORD_RULE_WEIGHT * (words * TOKENS_PER_WORD);
    (blended.round() as usize).max(1)
}
