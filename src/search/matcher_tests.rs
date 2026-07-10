//! Tests for the case-insensitive substring matcher — range correctness (original-string byte
//! offsets, char-boundary safety), non-overlap, and the case-fold edge cases.

use super::{contains, find_matches};

#[test]
fn empty_needle_matches_nothing() {
    assert!(find_matches("abc", "").is_empty());
}

#[test]
fn empty_needle_contains_everything() {
    assert!(contains("abc", ""));
    assert!(contains("", ""));
}

#[test]
fn simple_match_reports_byte_range() {
    assert_eq!(find_matches("hello world", "world"), vec![6..11]);
}

#[test]
fn case_insensitive_both_directions() {
    assert_eq!(find_matches("Hello World", "world"), vec![6..11]);
    assert_eq!(find_matches("hello world", "WORLD"), vec![6..11]);
    assert!(contains("EU-WEST", "eu"));
    assert!(contains("eu-west", "EU"));
}

#[test]
fn multiple_non_overlapping_matches() {
    assert_eq!(find_matches("abab", "ab"), vec![0..2, 2..4]);
}

#[test]
fn overlapping_candidates_advance_past_match() {
    // "aaa" with needle "aa": first match 0..2, scan resumes at 2 — no overlapping 1..3.
    assert_eq!(find_matches("aaa", "aa"), vec![0..2]);
}

#[test]
fn no_match_returns_empty() {
    assert!(find_matches("hello", "xyz").is_empty());
    assert!(!contains("hello", "xyz"));
}

#[test]
fn ranges_index_original_string_with_multibyte_prefix() {
    // 'é' is 2 bytes; the match after it must use ORIGINAL byte offsets.
    let hay = "café ROW";
    let ranges = find_matches(hay, "row");
    assert_eq!(ranges, vec![6..9]);
    assert_eq!(&hay[6..9], "ROW");
}

#[test]
fn uppercase_multibyte_needle_folds() {
    // 'É' (2 bytes) lowercases to 'é' (2 bytes) — same byte length, folded normally.
    let hay = "CAFÉ";
    let ranges = find_matches(hay, "café");
    assert_eq!(ranges, vec![0..5]);
    assert_eq!(&hay[0..5], "CAFÉ");
}

#[test]
fn length_changing_fold_is_skipped_not_crashed() {
    // 'İ' (U+0130, 2 bytes) lowercases to "i\u{307}" (3 bytes) — folding it would desync byte
    // offsets, so it is kept as-is: it matches itself but not plain "i".
    let hay = "İstanbul";
    assert!(contains(hay, "İ"));
    assert!(contains(hay, "stanbul"));
    assert!(!contains(hay, "i̇"));
    // Ranges after the unfolded char still land on char boundaries of the original.
    let ranges = find_matches(hay, "STANBUL");
    assert_eq!(ranges, vec![2..9]);
    assert_eq!(&hay[2..9], "stanbul");
}

#[test]
fn needle_longer_than_hay_no_match() {
    assert!(find_matches("ab", "abc").is_empty());
}

#[test]
fn contains_needle_longer_than_hay_is_false() {
    // Guards the ASCII fast-path length check and the non-ASCII fallback alike.
    assert!(!contains("ab", "abc"));
    assert!(!contains("é", "ébc"));
}

#[test]
fn contains_ascii_fast_path_matches_fold_fallback() {
    // The ASCII fast path must agree with the folding path on the same ASCII inputs.
    for (hay, needle, want) in [
        ("Hello World", "WORLD", true),
        ("Hello World", "xyz", false),
        ("ABCabc", "cA", true),
        ("", "a", false),
        ("a", "", true),
    ] {
        assert_eq!(contains(hay, needle), want, "contains({hay:?}, {needle:?})");
    }
}

#[test]
fn contains_ascii_hay_nonascii_needle_uses_fallback() {
    // A non-ASCII needle can never match inside an ASCII hay; the fallback returns false without
    // the fast path (which is skipped because the needle isn't ASCII).
    assert!(!contains("cafe", "é"));
}

#[test]
fn full_string_match() {
    assert_eq!(find_matches("abc", "ABC"), vec![0..3]);
}
