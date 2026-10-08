/// Unlike `combo_box`, which drops everything but ASCII letters and digits,
/// this keeps any alphanumeric character, so non-Latin text is searchable.
/// Case is folded; every non-alphanumeric character becomes a space.
pub(super) fn normalize(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect()
}

/// The words of a query, normalized.
pub(super) fn query_parts(query: &str) -> Vec<String> {
    normalize(query)
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// Whether a normalized `matcher` contains every word of the query.
pub(super) fn matches(matcher: &str, parts: &[String]) -> bool {
    parts.iter().all(|part| matcher.contains(part.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(option: &str, query: &str) -> bool {
        matches(&normalize(option), &query_parts(query))
    }

    #[test]
    fn non_ascii_and_case_are_searchable() {
        assert!(find("Zürich Altstadt", "ZÜRICH"));
        assert!(find("Ångström", "ångs"));
    }

    #[test]
    fn punctuation_splits_words() {
        assert!(find("Release 17.0.2 (Stable)", "17.0 stable"));
        assert!(find("Release 17.0.2 (Stable)", "stable 17"));
        assert!(!find("Release 17.0.2 (Stable)", "21"));
    }

    #[test]
    fn an_empty_query_matches_everything() {
        assert!(find("anything", "   "));
    }
}
