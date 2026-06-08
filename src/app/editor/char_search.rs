//! Character search (`f`/`F`/`t`/`T` and the `;`/`,` repeat) — the pure column-index math.
//!
//! Ported from jiq's `src/editor/char_search.rs`. ciq's query bar is effectively single-line for
//! vim purposes (multiline newlines are rare in SQL and vim char-search is a within-line motion in
//! both editors), so the search operates on one line's char vector and returns a **char column**;
//! the [`Editor`](crate::app::Editor) translates that to a textarea cursor move. Pure
//! `&str + col -> Option<col>`, so it earns the pure-core hard floor (`dev/core-modules.txt`).

/// Search direction: forward (`f`/`t`) scans right of the cursor, backward (`F`/`T`) left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    Forward,
    Backward,
}

impl SearchDirection {
    /// The opposite direction — used by `,` (repeat the last char-search reversed).
    pub fn opposite(self) -> Self {
        match self {
            SearchDirection::Forward => SearchDirection::Backward,
            SearchDirection::Backward => SearchDirection::Forward,
        }
    }
}

/// Search kind: `Find` (`f`/`F`) lands *on* the target; `Till` (`t`/`T`) lands one short of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchType {
    /// `f`/`F` — move to the target character itself.
    Find,
    /// `t`/`T` — move up to (just before/after) the target character.
    Till,
}

/// The last char-search, remembered for `;` (repeat) and `,` (repeat reversed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharSearchState {
    pub character: char,
    pub direction: SearchDirection,
    pub search_type: SearchType,
}

/// The char column index of the next/previous occurrence of `target` from `cursor_col` in `text`,
/// or `None` if absent in range. Forward scans `(cursor_col + 1..)`, backward scans `(0..cursor_col)`
/// reversed — the bare match index, **without** the `Find`/`Till` adjustment. The single source of
/// the char-scan both the motion path ([`find_char_position`]) and the operator path
/// (`vim::operator_char_range`) consume, so the scan lives in exactly one (hard-floor) place.
pub fn find_match_index(
    text: &str,
    cursor_col: usize,
    target: char,
    direction: SearchDirection,
) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    match direction {
        SearchDirection::Forward => {
            let search_start = cursor_col + 1;
            if search_start >= chars.len() {
                return None;
            }
            (search_start..chars.len()).find(|&i| chars[i] == target)
        }
        SearchDirection::Backward => {
            if cursor_col == 0 {
                return None;
            }
            (0..cursor_col).rev().find(|&i| chars[i] == target)
        }
    }
}

/// The new cursor **char column** for a char-search from `cursor_col` in `text`, or `None` if the
/// target isn't found in range. `Find` returns the target's column; `Till` stops one short (and is
/// clamped so it never moves backward past the cursor for a forward search, nor forward for a
/// backward one — an adjacent backward `T` stays put rather than landing on the target).
pub fn find_char_position(
    text: &str,
    cursor_col: usize,
    target: char,
    direction: SearchDirection,
    search_type: SearchType,
) -> Option<usize> {
    let i = find_match_index(text, cursor_col, target, direction)?;
    match (direction, search_type) {
        (_, SearchType::Find) => Some(i),
        (SearchDirection::Forward, SearchType::Till) => {
            Some(i.saturating_sub(1).max(cursor_col + 1))
        }
        // `T` lands one column *after* the target. When the target is adjacent (one column left of
        // the cursor) the destination `i + 1` equals `cursor_col`, so there is nowhere to move — vim
        // leaves the cursor put rather than stepping onto the target. `None` means "stay" (the
        // caller treats a missing position as a no-op).
        (SearchDirection::Backward, SearchType::Till) => (i + 1 < cursor_col).then_some(i + 1),
    }
}

#[cfg(test)]
#[path = "char_search_tests.rs"]
mod char_search_tests;
