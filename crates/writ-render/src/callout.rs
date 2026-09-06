//! Obsidian callouts: a blockquote whose first line is `> [!type]`.
//!
//! The syntax carries three things — a type, an optional fold marker and an
//! optional title — and this module reads all three out of that first line and
//! says what markup they become. It parses one line and formats two strings;
//! the blockquote it belongs to, and everything inside that blockquote, stays
//! the parser's job, so a callout nested in a list, a callout inside another
//! blockquote and a callout holding a table, a math span or a mermaid fence
//! all render through exactly the code that renders them anywhere else.

/// The twelve Obsidian callout types and the aliases each answers to.
///
/// The first entry of a row is the type as `data-callout` carries it and the
/// rest are the words that resolve to it. Case is folded before the lookup.
const TYPES: [&[&str]; 13] = [
    &["note"],
    &["abstract", "summary", "tldr"],
    &["info"],
    &["todo"],
    &["tip", "hint", "important"],
    &["success", "check", "done"],
    &["question", "help", "faq"],
    &["warning", "caution", "attention"],
    &["failure", "fail", "missing"],
    &["danger", "error"],
    &["bug"],
    &["example"],
    &["quote", "cite"],
];

/// The type an unrecognised word is styled as.
const FALLBACK_TYPE: &str = "note";

/// Whether a callout starts open or closed, and whether it folds at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fold {
    /// No marker after `]`: the callout does not fold.
    None,
    /// `+`: folds, and starts open.
    Open,
    /// `-`: folds, and starts closed.
    Closed,
}

impl Fold {
    /// The value `data-fold` carries.
    fn as_str(self) -> &'static str {
        match self {
            Fold::None => "none",
            Fold::Open => "open",
            Fold::Closed => "closed",
        }
    }
}

/// One callout's header, as authored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Callout {
    /// The word between `[!` and `]`, exactly as it was written. An
    /// unrecognised word is kept rather than replaced, so nothing the author
    /// typed is lost and a stylesheet can reach a type this build has never
    /// heard of.
    pub authored: String,
    /// The type the styling uses: the row `authored` resolves to, or
    /// [`FALLBACK_TYPE`] when it resolves to none.
    pub kind: &'static str,
    /// Whether the callout folds, and how it starts.
    pub fold: Fold,
    /// The rest of the first line, or the type name capitalised when there is
    /// no rest.
    pub title: String,
}

/// The callout `line` opens, or `None` when it opens none.
///
/// `line` is the first line inside the blockquote with the `>` markers already
/// stripped, which is how the parser hands it over. Anything but `[!` at the
/// start of the trimmed line is ordinary quoted text.
pub fn parse(line: &str) -> Option<Callout> {
    let rest = line.trim_start().strip_prefix("[!")?;
    let (authored, after) = rest.split_once(']')?;
    let authored = authored.trim();
    if authored.is_empty() {
        return None;
    }
    let (fold, title) = match after.strip_prefix('+') {
        Some(title) => (Fold::Open, title),
        None => match after.strip_prefix('-') {
            Some(title) => (Fold::Closed, title),
            None => (Fold::None, after),
        },
    };
    let kind = resolve_type(authored);
    let title = title.trim();
    let title = if title.is_empty() {
        capitalise(authored)
    } else {
        title.to_string()
    };
    Some(Callout {
        authored: authored.to_string(),
        kind,
        fold,
        title,
    })
}

/// The type a word resolves to, or [`FALLBACK_TYPE`] for a word no row names.
fn resolve_type(word: &str) -> &'static str {
    let lower = word.to_ascii_lowercase();
    TYPES
        .iter()
        .find(|row| row.iter().any(|name| *name == lower))
        .map_or(FALLBACK_TYPE, |row| row[0])
}

/// `word` with its first character upper-cased, which is the title a callout
/// carrying none shows.
fn capitalise(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// The markup that opens one callout: the wrapper and the title, up to the
/// point the blockquote's own content goes.
///
/// Both the authored type and the title are escaped, so a type or a title
/// written as markup is read as the text it is.
pub fn open_html(callout: &Callout) -> String {
    format!(
        "<div class=\"writ-callout\" data-callout=\"{authored}\" data-callout-type=\"{kind}\" data-fold=\"{fold}\">\
<div class=\"writ-callout-title\">{title}</div>\
<div class=\"writ-callout-body\">",
        authored = crate::escape_attribute(&callout.authored),
        kind = callout.kind,
        fold = callout.fold.as_str(),
        title = crate::escape_text(&callout.title),
    )
}

/// The markup that closes what [`open_html`] opened.
pub fn close_html() -> &'static str {
    "</div></div>"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_obsidian_type_keeps_its_own_name() {
        for row in TYPES {
            let callout = parse(&format!("[!{}]", row[0])).expect("a type opens a callout");
            assert_eq!(callout.kind, row[0]);
            assert_eq!(callout.authored, row[0]);
        }
    }

    #[test]
    fn an_alias_resolves_to_its_type_and_keeps_the_word_it_was_written_as() {
        let callout = parse("[!tldr]").expect("an alias opens a callout");
        assert_eq!(callout.kind, "abstract");
        assert_eq!(callout.authored, "tldr");
        assert_eq!(parse("[!hint]").unwrap().kind, "tip");
        assert_eq!(parse("[!cite]").unwrap().kind, "quote");
        assert_eq!(parse("[!error]").unwrap().kind, "danger");
    }

    #[test]
    fn a_type_no_row_names_falls_back_to_note_and_keeps_the_word() {
        let callout = parse("[!spaceship]").expect("an unknown type still opens a callout");
        assert_eq!(callout.kind, FALLBACK_TYPE);
        assert_eq!(callout.authored, "spaceship");
        assert_eq!(callout.title, "Spaceship");
    }

    #[test]
    fn the_type_is_read_whatever_case_it_is_written_in() {
        assert_eq!(parse("[!NOTE]").unwrap().kind, "note");
        assert_eq!(parse("[!Warning]").unwrap().kind, "warning");
        assert_eq!(parse("[!NOTE]").unwrap().authored, "NOTE");
    }

    #[test]
    fn a_fold_marker_says_whether_it_starts_open_or_closed() {
        assert_eq!(parse("[!tip]").unwrap().fold, Fold::None);
        assert_eq!(parse("[!tip]+").unwrap().fold, Fold::Open);
        assert_eq!(parse("[!tip]- Folded title").unwrap().fold, Fold::Closed);
        assert_eq!(parse("[!tip]- Folded title").unwrap().title, "Folded title");
    }

    #[test]
    fn the_title_is_the_rest_of_the_line_and_the_type_name_without_one() {
        assert_eq!(parse("[!note] Read this").unwrap().title, "Read this");
        assert_eq!(parse("[!note]").unwrap().title, "Note");
        assert_eq!(parse("[!note]   ").unwrap().title, "Note");
    }

    #[test]
    fn a_line_that_opens_no_callout_is_ordinary_quoted_text() {
        assert!(parse("just a quotation").is_none());
        assert!(parse("[!]").is_none());
        assert!(parse("[! ]").is_none());
        assert!(parse("[note]").is_none());
        assert!(parse("text [!note]").is_none());
    }

    #[test]
    fn a_type_or_a_title_written_as_markup_is_read_as_text() {
        let callout = parse("[!\"><script>] <script>alert(1)</script>").expect("callout");
        let html = open_html(&callout);
        assert!(!html.contains("<script>"));
        assert!(html.contains("data-callout=\"&quot;&gt;&lt;script&gt;\""));
        assert!(html.contains("data-callout-type=\"note\""));
    }
}
