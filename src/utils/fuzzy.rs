use strsim::jaro_winkler;

/// Represents a fuzzy match result.
#[derive(Debug, Clone)]
pub struct MatchResult<'a> {
    pub text: &'a str,
    pub score: f64,
}

/// Normalizes text for better media matching (e.g., "number one" -> "no. 1", "op" -> "op.").
#[must_use]
pub fn normalize_text(text: &str) -> String {
    let mut s = text.to_lowercase();
    let replacements = [
        ("number one", "no. 1"),
        ("number two", "no. 2"),
        ("number three", "no. 3"),
        ("number four", "no. 4"),
        ("number five", "no. 5"),
        ("number 1", "no. 1"),
        ("number 2", "no. 2"),
        ("number 3", "no. 3"),
        (" opus ", " op. "),
        (" op ", " op. "),
        ("simply", "symphony"),
    ];

    for (from, to) in replacements {
        s = s.replace(from, to);
    }
    s.trim().to_string()
}

/// Finds the best fuzzy matches for a search term in a list of candidates.
/// Ported from Python's difflib with modern Rust performance and safety.
#[must_use]
pub fn find_matches<'a>(
    search_term: &str,
    candidates: &'a [&'a str],
    limit: usize,
    threshold: f64,
) -> Vec<MatchResult<'a>> {
    let search_norm = normalize_text(search_term);
    let search_tokens: Vec<&str> = search_norm.split_whitespace().collect();

    let mut matches: Vec<MatchResult> = candidates
        .iter()
        .map(|&c| {
            let c_norm = normalize_text(c);

            // Tiered scoring:
            // 1. Exact match (after normalization) = 1.0
            // 2. Contains all tokens as whole words = 0.9 + small bonus
            // 3. Jaro-Winkler fuzzy score

            let mut score = jaro_winkler(&search_norm, &c_norm);

            if search_norm == c_norm {
                score = 1.0;
            } else if !search_tokens.is_empty()
                && search_tokens.iter().all(|&t| {
                    // Whole word check
                    c_norm.contains(t) && (c_norm.split_whitespace().any(|cw| cw == t))
                })
            {
                score = score.max(0.9);
            }

            MatchResult { text: c, score }
        })
        .filter(|m| m.score >= threshold)
        .collect();

    // Sort by score descending, then by text length ascending
    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.text.len().cmp(&b.text.len()))
    });

    matches.truncate(limit);
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_matches_exact() {
        let candidates = ["Pink Floyd", "The Beatles", "Led Zeppelin"];
        let results = find_matches("Pink Floyd", &candidates, 5, 0.6);
        assert_eq!(results[0].text, "Pink Floyd");
        assert!(results[0].score > 0.99);
    }

    #[test]
    fn test_find_matches_fuzzy() {
        let candidates = ["Symphony No. 5", "Symphony No. 9", "Piano Sonata"];
        let results = find_matches("Simphony 5", &candidates, 5, 0.7);
        assert_eq!(results[0].text, "Symphony No. 5");
    }
}
