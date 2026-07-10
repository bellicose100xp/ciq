//! Case-insensitive substring matching over cell text — the pure primitive the row filter and
//! the highlight painter share.
//!
//! Byte ranges are found on a lowercased copy but reported **against the original string**, so
//! they are safe to slice with. That only holds when lowercasing preserves byte offsets; for the
//! rare characters where it doesn't (e.g. `İ` lowercases to two chars), offset-preserving
//! per-char folding is used instead of `str::to_lowercase` — see [`fold_case_offset_preserving`].

use std::ops::Range;

/// All non-overlapping byte ranges in `hay` where `needle` matches case-insensitively, in
/// ascending order. Ranges index into the ORIGINAL `hay` and always land on char boundaries.
/// An empty needle matches nothing (an empty search filters nothing, highlights nothing).
pub fn find_matches(hay: &str, needle: &str) -> Vec<Range<usize>> {
    if needle.is_empty() {
        return Vec::new();
    }
    let hay_folded = fold_case_offset_preserving(hay);
    let needle_folded = fold_case_offset_preserving(needle);
    let mut out = Vec::new();
    let mut start = 0;
    while let Some(pos) = hay_folded[start..].find(needle_folded.as_str()) {
        let begin = start + pos;
        let end = begin + needle_folded.len();
        out.push(begin..end);
        start = end;
    }
    out
}

/// Whether `hay` contains `needle` case-insensitively. The row filter calls this once per cell
/// over the whole (uncapped) result, so it is the hot path: an allocating fold per cell turned a
/// 1M-row filter into a multi-second UI-thread stall. When both sides are ASCII — the
/// overwhelmingly common case for CSV data — it does a no-allocation ASCII-case-insensitive
/// substring scan, which is exactly equivalent to folding both (ASCII case-folding *is*
/// `to_ascii_lowercase`). Only genuinely non-ASCII text falls back to the offset-preserving fold.
pub fn contains(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if hay.is_ascii() && needle.is_ascii() {
        let n = needle.as_bytes();
        if n.len() > hay.len() {
            return false;
        }
        return hay
            .as_bytes()
            .windows(n.len())
            .any(|w| w.eq_ignore_ascii_case(n));
    }
    fold_case_offset_preserving(hay).contains(fold_case_offset_preserving(needle).as_str())
}

/// Lowercase `s` per char, keeping every char's byte offset identical to the original: a char
/// whose lowercase mapping has a different UTF-8 byte length (e.g. `İ` -> `i̇`) is kept as-is.
/// This trades exactness on those rare characters for the guarantee that a match range on the
/// folded string is a valid, boundary-aligned range on the original — which the highlighter
/// slices with.
fn fold_case_offset_preserving(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let lower = c.to_lowercase();
        if lower.clone().map(|l| l.len_utf8()).sum::<usize>() == c.len_utf8() {
            out.extend(lower);
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
#[path = "matcher_tests.rs"]
mod matcher_tests;
