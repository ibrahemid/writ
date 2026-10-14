# ADR-050: The spell-check engine

## Status

Accepted, 2026-10-14. Replaces Harper (`harper-core`) as the engine behind `writ-lint`; `writ_lint::check` keeps its signature.

## Context

`harper-core` pulled in 157 crates, among them the burn ML framework, ndarray, rayon and cubecl. Every check ran a part-of-speech tagger and a neural chunker over each sentence for rules Writ turns off, and the subtree carried three cargo-audit warnings (bincode, paste, lru). Every lint was `confident`, so Fix all pre-selected every guess, names included.

## Decision

1. **spellbook 0.4.2 on SCOWL dictionaries.** The SCOWL rel-2026.02.25 Hunspell dictionaries (size 60) ship unmodified in `crates/writ-lint/dictionaries/` with their READMEs. Each is parsed once, the first time its variant is used: American `en_US`, British `en_GB-ise` (SCOWL's `en_GB-ize` is the Oxford alternative), Canadian `en_CA`, Australian `en_AU`. `Dialect` is now writ-lint's own enum of these four; settings never reached Harper's fifth variant, Indian. `harper-words.txt` beside them holds the 26,606 words Harper's dictionary (harper-core 2.11.0, Apache-2.0) accepts and SCOWL rejects, each under the variants Harper gives it (frontend, localhost, Tauri's, configs), generated once from that crate and added to each dictionary as it is parsed.
2. **Suggestions.** Edit-based candidates come first. The ngram search over the whole word list costs tens of milliseconds a word, so it runs only when they find none, at most 32 times per `check` call; a word past that budget goes unreported in that call. The open-time check sends the whole document in one call, so a file with more than 32 such words shows the rest once their paragraphs are checked again. A lint keeps at most 3 suggestions, and a process-wide cache of 10,000 words keeps them between calls. A misspelling with no suggestion is still not reported.
3. **What is read.** pulldown-cmark runs with Harper's options and text containers: paragraph, heading, list item, table cell, emphasis, strong, strikethrough, link text. Code, HTML, math, front matter, image alt text, autolinks, email links and the target of an unpiped wikilink are skipped, as are whitespace-separated URLs, email addresses, host names, paths and HTML entities. Hyphens split words; apostrophes (`'`, `’`) inside a word stay in it. Spelling judges words in ASCII letters only, so Arabic, Cyrillic, CJK and accented words are never flagged; the dictionaries are built with accents stripped.
4. **Mechanical rules, in-house.** Repeated word: whitespace alone between, Latin letters, not joined by a hyphen, not laughter, not a legitimate double (had, that, bye, blah, knock, tsk, yada). Repeated spaces: not after the end of a sentence, never leading or trailing. A space before a final `.`, `!` or `?`. A semicolon, acute accent or backtick inside a contraction. a/an, ported from Harper with its tests under Apache-2.0, Harper's licence, with attribution in `an_a.rs` and THIRD-PARTY-NOTICES.md. Kind keys are Harper's, and so are the messages except the apostrophe one, which keeps the typed case.
5. **`confident`.** Always true for the mechanical rules. For spelling, true only when the dictionary offers exactly one suggestion and it is one edit away by optimal string alignment distance: a substitution, an insertion, a deletion, or a swap of two adjacent letters.

## Consequences

- writ-lint's own dependencies are spellbook and pulldown-cmark. The lockfile loses 268 packages and the shipped set 94 crates; bincode, paste and lru leave cargo audit.
- Without a part-of-speech tagger, a noun-verb double such as "address address" is flagged, and "had had" and "that that" are not.
- The dictionaries and the Harper list add about 2.3 MB of text to the binary. The first check in a variant pays a parse of about 15 ms. The list makes each ngram search slower, from 16 to 24 ms in a release build, so a check that spends its whole budget of 32 takes up to about 0.8 s.
- Names neither list has are flagged, and a name with a single suggestion one edit away is confident. Ignore covers both.
