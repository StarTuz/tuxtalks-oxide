use crate::utils::fuzzy::find_matches;

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_debug_snapshot;

    #[test]
    fn test_fuzzy_matching_snapshots() {
        let candidates = [
            "Dark Side of the Moon",
            "The Wall",
            "Wish You Were Here",
            "Animals",
            "Meddle",
        ];

        let match_1 = find_matches("Dark Side", &candidates, 3, 0.6);
        assert_debug_snapshot!("fuzzy_match_dark_side", match_1);

        let match_2 = find_matches("Wall", &candidates, 3, 0.6);
        assert_debug_snapshot!("fuzzy_match_wall", match_2);

        let match_3 = find_matches("Animal", &candidates, 3, 0.6);
        assert_debug_snapshot!("fuzzy_match_animals", match_3);
    }
}
