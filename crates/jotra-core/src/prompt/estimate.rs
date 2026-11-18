const CHAR_RULE_WEIGHT: f64 = 0.6;
const WORD_RULE_WEIGHT: f64 = 0.4;
const CHARS_PER_TOKEN: f64 = 4.0;
const TOKENS_PER_WORD: f64 = 4.0 / 3.0;
/// Tokens a Han, kana or Hangul character costs: about one each.
const TOKENS_PER_CJK_CHAR: f64 = 1.0;
/// Tokens an Arabic-script character costs: about one per two.
const TOKENS_PER_ARABIC_CHAR: f64 = 0.5;

/// The scripts a tokenizer spends far more than a quarter token per character
/// on, which the character rule would undercount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DenseScript {
    /// Han ideographs, Hiragana, Katakana and Hangul.
    Cjk,
    /// The Arabic script, its supplements and presentation forms.
    Arabic,
}

/// The dense script `c` belongs to, or `None` for every other character.
///
/// `src/editor/document-counts.ts` classifies the same ranges, so the status
/// bar and this estimate count the same characters.
pub(crate) fn dense_script(c: char) -> Option<DenseScript> {
    match u32::from(c) {
        0x1100..=0x11FF
        | 0x3040..=0x30FF
        | 0x3130..=0x318F
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xAC00..=0xD7AF
        | 0xF900..=0xFAFF
        | 0x20000..=0x2FFFF => Some(DenseScript::Cjk),
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFC => {
            Some(DenseScript::Arabic)
        }
        _ => None,
    }
}

/// Estimates the LLM token count of `text` using a blended heuristic.
///
/// Blends the ~4-characters-per-token and ~1.33-tokens-per-word rules of
/// thumb, weighted toward the character rule, which tracks mixed
/// prose/code buffers more reliably. For English prose, Markdown, and
/// mainstream code the result lands within roughly ±30% of cl100k-family
/// tokenizers.
///
/// Characters of a dense script ([`dense_script`]) leave the character rule
/// and are counted at their own rate: a Han, kana or Hangul character as one
/// token, an Arabic one as half a token. The quarter-token rate undercounts
/// those scripts two to four times. Text with none of them estimates exactly
/// as the blend alone does.
///
/// Whitespace-only input estimates to zero. Non-empty trimmed input always
/// estimates to at least one token.
pub fn estimate_tokens(text: &str) -> usize {
    if text.trim().is_empty() {
        return 0;
    }
    let mut chars = 0usize;
    let mut cjk = 0usize;
    let mut arabic = 0usize;
    for c in text.chars() {
        chars += 1;
        match dense_script(c) {
            Some(DenseScript::Cjk) => cjk += 1,
            Some(DenseScript::Arabic) => arabic += 1,
            None => {}
        }
    }
    let words = text.split_whitespace().count();
    estimate_from_counts(words, chars, cjk, arabic)
}

/// [`estimate_tokens`] from the counts a document already holds: `words` runs
/// between whitespace, `chars` code points, and how many of those are
/// [`DenseScript::Cjk`] and [`DenseScript::Arabic`].
///
/// The order of the arithmetic is `src/lib/token-estimate.ts`'s, so the two
/// round the same sums the same way.
fn estimate_from_counts(words: usize, chars: usize, cjk: usize, arabic: usize) -> usize {
    if words == 0 {
        return 0;
    }
    let other = chars.saturating_sub(cjk + arabic) as f64;
    let blended = CHAR_RULE_WEIGHT * (other / CHARS_PER_TOKEN)
        + WORD_RULE_WEIGHT * (words as f64 * TOKENS_PER_WORD)
        + TOKENS_PER_CJK_CHAR * cjk as f64
        + TOKENS_PER_ARABIC_CHAR * arabic as f64;
    (blended.round() as usize).max(1)
}
