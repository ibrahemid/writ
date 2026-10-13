// writ_core::prompt::estimate_tokens, from the counts the status bar already
// holds rather than from the text. crates/writ-core/tests/fixtures/token-estimates.json
// holds the two to the same answers.
const CHAR_RULE_WEIGHT = 0.6;
const WORD_RULE_WEIGHT = 0.4;
const CHARS_PER_TOKEN = 4;
const TOKENS_PER_WORD = 4 / 3;

export interface TokenEstimateInput {
  words: number;
  characters: number;
}

/** The LLM token estimate for a text with these counts; 0 for a text with no words. */
export function estimateTokens(counts: TokenEstimateInput): number {
  if (counts.words === 0) return 0;
  const blended =
    CHAR_RULE_WEIGHT * (counts.characters / CHARS_PER_TOKEN) +
    WORD_RULE_WEIGHT * (counts.words * TOKENS_PER_WORD);
  return Math.max(1, Math.round(blended));
}

export function formatTokenCount(count: number): string {
  if (count < 1000) return String(count);
  if (count >= 1_000_000) {
    const millions = count / 1_000_000;
    const rounded = Math.round(millions * 10) / 10;
    if (millions < 10 && rounded < 10) {
      return Number.isInteger(rounded) ? `${rounded}M` : `${rounded.toFixed(1)}M`;
    }
    return `${Math.round(millions)}M`;
  }
  const thousands = count / 1000;
  if (thousands < 10) {
    const rounded = Math.round(thousands * 10) / 10;
    if (rounded >= 10) return "10k";
    return Number.isInteger(rounded) ? `${rounded}k` : `${rounded.toFixed(1)}k`;
  }
  const rounded = Math.round(thousands);
  return rounded >= 1000 ? "1M" : `${rounded}k`;
}
