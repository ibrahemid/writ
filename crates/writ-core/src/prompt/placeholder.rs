use std::collections::{HashMap, HashSet};

/// Returns the placeholder names found in `text`, deduplicated and in
/// first-occurrence order.
///
/// A placeholder is `{{identifier}}` where the identifier starts with a
/// Unicode letter or underscore and continues with Unicode alphanumerics
/// or underscores. Escaped openers (`\{{`) and malformed slots (invalid
/// identifier, unbalanced braces) are ignored.
pub fn scan_placeholders(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (_, _, name) in parse_placeholders(text) {
        if seen.insert(name) {
            out.push(name.to_string());
        }
    }
    out
}

/// Replaces every `{{name}}` occurrence in `text` with `values[name]`.
///
/// Placeholders without a supplied value are left intact. Escaped openers
/// (`\{{`) are never touched. Substituted values are inserted literally
/// and never rescanned for further placeholders.
pub fn fill_placeholders(text: &str, values: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (start, end, name) in parse_placeholders(text) {
        if let Some(value) = values.get(name) {
            out.push_str(&text[last..start]);
            out.push_str(value);
            last = end;
        }
    }
    out.push_str(&text[last..]);
    out
}

fn parse_placeholders(text: &str) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if rest.starts_with("\\{{") {
            i += 3;
            continue;
        }
        if let Some(after_open) = rest.strip_prefix("{{") {
            if let Some((name_len, name)) = parse_identifier(after_open) {
                if after_open[name_len..].starts_with("}}") {
                    let end = i + 2 + name_len + 2;
                    out.push((i, end, name));
                    i = end;
                    continue;
                }
            }
            i += 1;
            continue;
        }
        i += rest.chars().next().map_or(1, char::len_utf8);
    }
    out
}

fn parse_identifier(s: &str) -> Option<(usize, &str)> {
    let mut chars = s.char_indices();
    let (_, first) = chars.next()?;
    if !(first.is_alphabetic() || first == '_') {
        return None;
    }
    let mut end = first.len_utf8();
    for (idx, c) in chars {
        if c.is_alphanumeric() || c == '_' {
            end = idx + c.len_utf8();
        } else {
            break;
        }
    }
    Some((end, &s[..end]))
}
