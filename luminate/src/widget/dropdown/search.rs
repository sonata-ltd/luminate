/// Unlike `combo_box`, which drops everything but ASCII letters and digits,
/// this keeps any alphanumeric character, so non-Latin text is searchable.
/// Case is optionally folded; every non-alphanumeric character becomes a space.
pub(super) fn normalize(text: &str, case_sensitive: bool) -> String {
    let text = if case_sensitive {
        text.to_owned()
    } else {
        text.to_lowercase()
    };
    text.chars()
        .map(|c| match c {
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect()
}

/// The words of a query, normalized.
pub(super) fn query_parts(query: &str, case_sensitive: bool) -> Vec<String> {
    normalize(query, case_sensitive)
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// Exact comparison preserves punctuation and whitespace in search keys.
pub(super) fn exact_match(key: &str, query: &str, case_sensitive: bool) -> bool {
    !query.is_empty()
        && if case_sensitive {
            key == query
        } else {
            key.to_lowercase() == query.to_lowercase()
        }
}

/// Whether a normalized `matcher` contains every word of the query.
pub(super) fn matches(matcher: &str, parts: &[String]) -> bool {
    parts.iter().all(|part| matcher.contains(part.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(option: &str, query: &str) -> bool {
        matches(&normalize(option, false), &query_parts(query, false))
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

    #[test]
    fn case_sensitive_filtering_preserves_case() {
        let key = normalize("Zürich Altstadt", true);
        assert!(matches(&key, &query_parts("Zürich", true)));
        assert!(!matches(&key, &query_parts("ZÜRICH", true)));
    }

    #[test]
    fn exact_matching_preserves_punctuation_and_rejects_partial_queries() {
        assert!(exact_match("Zürich", "ZÜRICH", false));
        assert!(exact_match("City", "CITY", false));
        assert!(!exact_match("Zürich", "ZÜRICH", true));
        assert!(!exact_match("Release 1.0", "Release 1 0", false));
        assert!(!exact_match("Berlin", "Berl", false));
        assert!(!exact_match("", "", false));
    }
}
