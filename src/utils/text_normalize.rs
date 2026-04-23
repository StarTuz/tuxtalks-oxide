//! Text normalization primitives.
//!
//! Mirrors Python `core/text_normalizer.py`. Kept pure (no I/O, no config
//! loading) so it is useful in both the media CLI (parsing "goto five")
//! and in the eventual voice pipeline rework.
//!
//! The Python `TextNormalizer` loads alias maps from several JSON sources
//! with a priority system. That's deferred — voice work will bring it back
//! when the full ASR → command-routing path is ported. For now the normalizer
//! takes any alias map the caller wants to apply.

use std::collections::BTreeMap;

/// Alias map: spoken (mis-)phrase → intended phrase. Both sides lowercase.
pub type Aliases = BTreeMap<String, String>;

/// Apply an alias map and (optionally) strip a leading wake word.
///
/// `listening_mode=true` mirrors Python `state == 0` — the wake word is kept
/// because downstream logic uses it to trigger state transitions. In any
/// other state, a leading `"<wake_word> "` prefix is stripped.
///
/// Aliases are applied case-insensitively at word boundaries, same as the
/// Python `re.sub(r'\b<pat>\b', repl, text)` loop. Trailing punctuation /
/// spaces are trimmed, matching Python's `text.strip(".,!?;: ")`.
#[must_use]
pub fn normalize(
    text: &str,
    aliases: &Aliases,
    listening_mode: bool,
    wake_word: Option<&str>,
) -> String {
    let mut out = text.to_lowercase();

    for (wrong, right) in aliases {
        out = replace_word_bounded(&out, wrong, right);
    }

    if !listening_mode {
        if let Some(ww) = wake_word {
            let ww_lower = ww.to_lowercase();
            let prefix = format!("{ww_lower} ");
            if out.starts_with(&prefix) {
                out = out[prefix.len()..].to_string();
            }
        }
    }

    out.trim_matches(|c: char| matches!(c, '.' | ',' | '!' | '?' | ';' | ':' | ' '))
        .to_string()
}

/// Word-boundary-aware replace. "Word" = ASCII alphanumeric or `_`, matching
/// Python's default `\b` behavior for the strings in the Python alias map
/// (which are ASCII-only).
fn replace_word_bounded(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || !haystack.contains(needle) {
        return haystack.to_string();
    }

    let bytes = haystack.as_bytes();
    let needle_bytes = needle.as_bytes();
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;

    while i < bytes.len() {
        if i + needle_bytes.len() <= bytes.len()
            && &bytes[i..i + needle_bytes.len()] == needle_bytes
        {
            let before_ok = i == 0 || !is_word_byte(bytes[i - 1]);
            let after_idx = i + needle_bytes.len();
            let after_ok = after_idx == bytes.len() || !is_word_byte(bytes[after_idx]);
            if before_ok && after_ok {
                out.push_str(replacement);
                i = after_idx;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }

    out
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Parse a spoken number 0–99 (plus digit strings) from free text.
///
/// Mirrors Python `TextNormalizer.parse_number`:
/// - scans whitespace-separated words left-to-right
/// - strips trailing `.,!?;:`
/// - handles homophones (`for` → `four`, `to`/`too` → `two`, `night` → `eight`, etc.)
/// - combines tens + ones (`twenty one` = 21) when a tens word is followed by a ones word
/// - returns the **last** recognized number in the text if multiple appear,
///   except that `thirty five` stays 35 (tens+ones is a single accumulation).
#[must_use]
pub fn parse_number(text: &str) -> Option<u64> {
    let text = text.to_lowercase();

    let num_map: &[(&str, u64)] = &[
        ("zero", 0),
        ("one", 1),
        ("two", 2),
        ("three", 3),
        ("four", 4),
        ("five", 5),
        ("six", 6),
        ("seven", 7),
        ("eight", 8),
        ("nine", 9),
        ("ten", 10),
        ("eleven", 11),
        ("twelve", 12),
        ("thirteen", 13),
        ("fourteen", 14),
        ("fifteen", 15),
        ("sixteen", 16),
        ("seventeen", 17),
        ("eighteen", 18),
        ("nineteen", 19),
        ("twenty", 20),
        ("thirty", 30),
        ("forty", 40),
        ("fifty", 50),
        ("sixty", 60),
        ("seventy", 70),
        ("eighty", 80),
        ("ninety", 90),
    ];

    let homophones: &[(&str, &str)] = &[
        ("for", "four"),
        ("to", "two"),
        ("too", "two"),
        ("tree", "three"),
        ("ate", "eight"),
        ("won", "one"),
        ("sea", "three"),
        ("sex", "six"),
        ("night", "eight"),
    ];

    let mut current: u64 = 0;
    let mut found = false;

    for raw in text.split_whitespace() {
        let word = raw.trim_matches(|c: char| matches!(c, '.' | ',' | '!' | '?' | ';' | ':'));
        if word.is_empty() {
            continue;
        }

        let word = homophones
            .iter()
            .find_map(|(k, v)| if *k == word { Some(*v) } else { None })
            .unwrap_or(word);

        let val: Option<u64> = if word.chars().all(|c| c.is_ascii_digit()) {
            word.parse().ok()
        } else {
            num_map.iter().find_map(|(k, v)| (*k == word).then_some(*v))
        };

        if let Some(v) = val {
            if v < 100 {
                if current >= 20 && current.is_multiple_of(10) && v < 10 {
                    current += v;
                } else {
                    current = v;
                }
                found = true;
            }
        }
    }

    found.then_some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aliases(pairs: &[(&str, &str)]) -> Aliases {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn normalize_lowercases_and_strips_punct() {
        let a = Aliases::new();
        assert_eq!(
            normalize("Play Beethoven.", &a, true, None),
            "play beethoven"
        );
    }

    #[test]
    fn normalize_applies_aliases_with_word_boundary() {
        let a = aliases(&[("von", "vaughan")]);
        assert_eq!(
            normalize("play von williams", &a, true, None),
            "play vaughan williams"
        );
    }

    #[test]
    fn normalize_does_not_replace_inside_other_words() {
        // "von" appearing inside "beethoven" must not be rewritten.
        let a = aliases(&[("von", "vaughan")]);
        assert_eq!(
            normalize("play beethoven", &a, true, None),
            "play beethoven"
        );
    }

    #[test]
    fn normalize_strips_wake_word_only_when_not_listening() {
        let a = Aliases::new();
        assert_eq!(
            normalize("alice play", &a, true, Some("alice")),
            "alice play",
            "listening mode keeps wake word"
        );
        assert_eq!(
            normalize("alice play", &a, false, Some("alice")),
            "play",
            "non-listening strips wake word"
        );
        assert_eq!(
            normalize("alicia play", &a, false, Some("alice")),
            "alicia play",
            "prefix match only, not substring"
        );
    }

    #[test]
    fn parse_number_digits() {
        assert_eq!(parse_number("5"), Some(5));
        assert_eq!(parse_number("go to track 12"), Some(12));
    }

    #[test]
    fn parse_number_single_word() {
        assert_eq!(parse_number("five"), Some(5));
        assert_eq!(parse_number("zero"), Some(0));
        assert_eq!(parse_number("twelve"), Some(12));
    }

    #[test]
    fn parse_number_tens_and_ones() {
        assert_eq!(parse_number("twenty one"), Some(21));
        assert_eq!(parse_number("thirty five"), Some(35));
        assert_eq!(parse_number("ninety nine"), Some(99));
    }

    #[test]
    fn parse_number_homophones() {
        assert_eq!(parse_number("for"), Some(4));
        assert_eq!(parse_number("track night"), Some(8));
    }

    #[test]
    fn parse_number_ignores_non_numbers() {
        assert_eq!(parse_number("play beethoven"), None);
        assert_eq!(parse_number(""), None);
    }

    #[test]
    fn parse_number_last_wins_for_non_tens_pair() {
        // "five ten" → current=5, then ten resets current to 10 (not 15).
        assert_eq!(parse_number("five ten"), Some(10));
    }
}
