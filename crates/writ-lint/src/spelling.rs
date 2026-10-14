//! Spelling against the bundled Hunspell dictionaries and the words Harper's
//! dictionary adds to them.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use spellbook::Dictionary;

use crate::rules::is_informal_laughter;
use crate::tokens::{is_latin_word, Token, TokenKind};
use crate::{Dialect, Finding, LintConfig};

/// The most suggestions a spelling lint carries.
const MAX_SUGGESTIONS: usize = 3;

/// The most words whose suggestions are remembered between calls. The cache
/// is emptied when it fills.
const CACHE_CAPACITY: usize = 10_000;

type SuggestionCache = HashMap<(Dialect, String), Vec<String>>;

static SUGGESTION_CACHE: OnceLock<Mutex<SuggestionCache>> = OnceLock::new();

const KIND: &str = "Spelling";

struct Bundled {
    cell: &'static OnceLock<Option<Dictionary>>,
    aff: &'static str,
    dic: &'static str,
    /// The variant's name in the headers of [`HARPER_WORDS`].
    tag: &'static str,
}

/// Words the dictionary of harper-core 2.11.0 accepts that the SCOWL
/// dictionary of a variant rejects. A bracketed line names the variants the
/// words after it belong to.
const HARPER_WORDS: &str = include_str!("../dictionaries/harper-words.txt");

/// The words of [`HARPER_WORDS`] listed for `dialect`.
pub(crate) fn harper_words(dialect: Dialect) -> impl Iterator<Item = &'static str> {
    let tag = bundled(dialect).tag;
    let mut listed = false;
    HARPER_WORDS.lines().filter(move |line| {
        if line.is_empty() || line.starts_with('#') {
            return false;
        }
        if let Some(tags) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            listed = tags.split(' ').any(|t| t == tag);
            return false;
        }
        listed
    })
}

static AMERICAN: OnceLock<Option<Dictionary>> = OnceLock::new();
static BRITISH: OnceLock<Option<Dictionary>> = OnceLock::new();
static CANADIAN: OnceLock<Option<Dictionary>> = OnceLock::new();
static AUSTRALIAN: OnceLock<Option<Dictionary>> = OnceLock::new();

fn bundled(dialect: Dialect) -> Bundled {
    match dialect {
        Dialect::American => Bundled {
            cell: &AMERICAN,
            aff: include_str!("../dictionaries/en_US.aff"),
            dic: include_str!("../dictionaries/en_US.dic"),
            tag: "US",
        },
        Dialect::British => Bundled {
            cell: &BRITISH,
            aff: include_str!("../dictionaries/en_GB-ise.aff"),
            dic: include_str!("../dictionaries/en_GB-ise.dic"),
            tag: "GB",
        },
        Dialect::Canadian => Bundled {
            cell: &CANADIAN,
            aff: include_str!("../dictionaries/en_CA.aff"),
            dic: include_str!("../dictionaries/en_CA.dic"),
            tag: "CA",
        },
        Dialect::Australian => Bundled {
            cell: &AUSTRALIAN,
            aff: include_str!("../dictionaries/en_AU.aff"),
            dic: include_str!("../dictionaries/en_AU.dic"),
            tag: "AU",
        },
    }
}

/// The dictionary for `dialect` with its [`harper_words`] added, parsed on
/// first use and kept for the life of the process. `None` only if the bundled
/// files fail to parse, which the crate's tests rule out for every dialect.
pub(crate) fn dictionary(dialect: Dialect) -> Option<&'static Dictionary> {
    let bundled = bundled(dialect);
    bundled
        .cell
        .get_or_init(|| {
            let mut dictionary = Dictionary::new(bundled.aff, bundled.dic).ok()?;
            for word in harper_words(dialect) {
                dictionary.add(word).ok()?;
            }
            Some(dictionary)
        })
        .as_ref()
}

/// The most ngram searches one [`crate::check`] call runs. Each costs tens of
/// milliseconds; a misspelling past the limit is left unreported by that call
/// and gets its suggestions when a later call reaches it.
pub(crate) const NGRAM_SEARCHES_PER_CHECK: usize = 32;

/// Corrections for `word`, most likely first. Edit-based candidates come
/// first; the slow ngram search over the whole word list runs only when they
/// find nothing and `ngram_budget` allows, which it then decrements. `None`
/// when the search was needed but the budget was spent. Results are
/// remembered per dialect between calls.
pub(crate) fn suggestions_for(
    dictionary: &Dictionary,
    dialect: Dialect,
    word: &str,
    ngram_budget: &mut usize,
) -> Option<Vec<String>> {
    let cache = SUGGESTION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (dialect, word.to_owned());
    if let Some(hit) = cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
    {
        return Some(hit.clone());
    }

    let mut found = Vec::new();
    dictionary
        .suggester()
        .with_ngram_suggestions(false)
        .suggest(word, &mut found);
    if found.is_empty() {
        if *ngram_budget == 0 {
            return None;
        }
        *ngram_budget -= 1;
        dictionary.suggest(word, &mut found);
    }

    let mut cache = cache.lock().unwrap_or_else(PoisonError::into_inner);
    if cache.len() >= CACHE_CAPACITY {
        cache.clear();
    }
    cache.insert(key, found.clone());
    Some(found)
}

/// True when a token carries an uppercase letter anywhere but the first
/// position: the shape of ALLCAPS acronyms (`API`) and camel or Pascal case
/// identifiers (`SolidJS`, `useSignal`) that spell check must not flag.
pub(crate) fn has_internal_uppercase(word: &str) -> bool {
    word.chars().skip(1).any(char::is_uppercase)
}

/// True when `word` is on the ignore list, matching the stored form
/// case-sensitively first, then case-insensitively.
pub(crate) fn is_ignored(word: &str, ignored: &[String]) -> bool {
    if ignored.iter().any(|w| w == word) {
        return true;
    }
    let lower = word.to_lowercase();
    ignored.iter().any(|w| w.to_lowercase() == lower)
}

/// True when `a` becomes `b` through exactly one edit: one character
/// substituted, inserted or deleted, or two adjacent characters swapped
/// (optimal string alignment distance 1).
pub(crate) fn is_one_edit_apart(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a == b {
        return false;
    }
    if a.len() == b.len() {
        let diffs: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
        return match diffs.as_slice() {
            [_] => true,
            [i, j] => *j == i + 1 && a[*i] == b[*j] && a[*j] == b[*i],
            _ => false,
        };
    }
    if a.len().abs_diff(b.len()) != 1 {
        return false;
    }
    let (short, long) = if a.len() < b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    let prefix = short
        .iter()
        .zip(long.iter())
        .take_while(|(x, y)| x == y)
        .count();
    short[prefix..] == long[prefix + 1..]
}

/// Flags misspelled words among `tokens`, skipping any word whose range lies
/// inside one of `covered` (a mechanical lint already owns that text).
pub(crate) fn lint(
    text: &str,
    tokens: &[Token],
    config: &LintConfig,
    covered: &[(usize, usize)],
    out: &mut Vec<Finding>,
) {
    let Some(dictionary) = dictionary(config.dialect) else {
        return;
    };
    let mut ngram_budget = NGRAM_SEARCHES_PER_CHECK;
    for token in tokens {
        if token.kind != TokenKind::Word {
            continue;
        }
        let word = token.text(text);
        if !is_latin_word(word)
            || has_internal_uppercase(word)
            || is_informal_laughter(word)
            || is_ignored(word, &config.ignored_words)
        {
            continue;
        }
        if covered
            .iter()
            .any(|&(from, to)| from <= token.start && token.end <= to)
        {
            continue;
        }
        if dictionary.check(word) {
            continue;
        }

        let Some(all) = suggestions_for(dictionary, config.dialect, word, &mut ngram_budget) else {
            continue;
        };
        if all.is_empty() {
            continue;
        }
        let confident = all.len() == 1 && is_one_edit_apart(word, &all[0]);
        let suggestions: Vec<String> = all.iter().take(MAX_SUGGESTIONS).cloned().collect();
        let message = if suggestions.len() == 1 {
            format!("Did you mean `{}`?", suggestions[0])
        } else {
            format!("Did you mean to spell `{word}` this way?")
        };
        out.push(Finding {
            start: token.start,
            end: token.end,
            kind: KIND,
            message,
            suggestions,
            confident,
        });
    }
}
