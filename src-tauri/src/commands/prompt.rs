use std::collections::HashMap;

#[tauri::command]
pub fn prompt_estimate_tokens(text: String) -> usize {
    writ_core::prompt::estimate_tokens(&text)
}

#[tauri::command]
pub fn prompt_scan_placeholders(text: String) -> Vec<String> {
    writ_core::prompt::scan_placeholders(&text)
}

#[tauri::command]
pub fn prompt_fill_placeholders(text: String, values: HashMap<String, String>) -> String {
    writ_core::prompt::fill_placeholders(&text, &values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_returns_zero_for_empty_text() {
        assert_eq!(prompt_estimate_tokens(String::new()), 0);
    }

    #[test]
    fn estimate_tokens_returns_positive_for_prose() {
        let est = prompt_estimate_tokens("The quick brown fox jumps over the lazy dog.".into());
        assert!(est > 0);
    }

    #[test]
    fn scan_placeholders_returns_ordered_unique_names() {
        let names = prompt_scan_placeholders("{{b}} {{a}} {{b}}".into());
        assert_eq!(names, vec!["b".to_string(), "a".to_string()]);
    }

    #[test]
    fn fill_placeholders_substitutes_supplied_values() {
        let values = HashMap::from([("name".to_string(), "Writ".to_string())]);
        let filled = prompt_fill_placeholders("hello {{name}}".into(), values);
        assert_eq!(filled, "hello Writ");
    }

    #[test]
    fn fill_placeholders_leaves_unknown_slots_intact() {
        let filled = prompt_fill_placeholders("hello {{name}}".into(), HashMap::new());
        assert_eq!(filled, "hello {{name}}");
    }
}
