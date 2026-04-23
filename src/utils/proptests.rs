use crate::utils::fuzzy::find_matches;
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_fuzzy_matching_never_panics(
        query in ".*",
        candidates in prop::collection::vec(".*", 0..10)
    ) {
        let cand_refs: Vec<&str> = candidates.iter().map(std::string::String::as_str).collect();
        // We don't care about the result, just that it doesn't crash on random unicode/junk
        let _ = find_matches(&query, &cand_refs, 5, 0.6);
    }

    #[test]
    fn test_threshold_enforcement(
        score in 0.0..1.0f64
    ) {
        let candidates = ["Testing 123"];
        let results = find_matches("Test", &candidates, 1, score);
        for r in results {
            assert!(r.score >= score);
        }
    }
}
