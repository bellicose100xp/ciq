//! Shared text-matching primitives — the one home for the fuzzy-filter rule both the autocomplete
//! ranker and the column palette use.
//!
//! Neutral top-level leaf (sibling of [`sql_ident`](crate::sql_ident)) so the palette can apply the
//! same case-insensitive subsequence match the autocomplete ranker does **without importing the
//! autocomplete module** — exactly the cross-module coupling the §0/D2 swappable-box discipline
//! avoids, and the same move already made for `quote_ident`. Having one implementation means the
//! two fuzzy filters can never drift in ranking semantics.

/// Whether `needle` is a (not-necessarily-contiguous) subsequence of `hay`, in order. **Both must
/// already be lowercased** by the caller (the candidate ranker and the palette filter both
/// case-fold first). An empty `needle` is vacuously a subsequence.
pub fn is_subsequence(hay: &str, needle: &str) -> bool {
    let mut needle_chars = needle.chars().peekable();
    for hc in hay.chars() {
        match needle_chars.peek() {
            None => return true,
            Some(&nc) if nc == hc => {
                needle_chars.next();
            }
            _ => {}
        }
    }
    needle_chars.peek().is_none()
}

#[cfg(test)]
#[path = "text_match_tests.rs"]
mod text_match_tests;
