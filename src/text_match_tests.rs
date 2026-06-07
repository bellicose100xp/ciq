//! Tests for the shared subsequence matcher (`text_match.rs`). These cover the cases both the
//! autocomplete ranker and the palette filter rely on — the single implementation they now share.

use super::is_subsequence;

#[test]
fn empty_needle_matches_anything() {
    assert!(is_subsequence("anything", ""));
    assert!(is_subsequence("", ""));
}

#[test]
fn empty_hay_only_matches_empty_needle() {
    assert!(!is_subsequence("", "a"));
}

#[test]
fn contiguous_prefix_is_a_subsequence() {
    assert!(is_subsequence("created_at", "cre"));
}

#[test]
fn non_contiguous_subsequence_matches_in_order() {
    // c..a..t appears in order in "created_at".
    assert!(is_subsequence("created_at", "cat"));
}

#[test]
fn out_of_order_chars_do_not_match() {
    assert!(!is_subsequence("created_at", "tac"));
}

#[test]
fn missing_char_does_not_match() {
    assert!(!is_subsequence("abc", "abd"));
}

#[test]
fn full_string_is_a_subsequence_of_itself() {
    assert!(is_subsequence("region", "region"));
}
