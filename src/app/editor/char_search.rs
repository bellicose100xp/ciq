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

/// The new cursor **char column** for a char-search from `cursor_col` in `text`, or `None` if the
/// target isn't found in range. `Find` returns the target's column; `Till` stops one short (and is
/// clamped so it never moves backward past the cursor for a forward search, nor forward for a
/// backward one).
pub fn find_char_position(
    text: &str,
    cursor_col: usize,
    target: char,
    direction: SearchDirection,
    search_type: SearchType,
) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    match direction {
        SearchDirection::Forward => {
            let search_start = cursor_col + 1;
            if search_start >= chars.len() {
                return None;
            }
            chars
                .iter()
                .enumerate()
                .skip(search_start)
                .find(|&(_, &ch)| ch == target)
                .map(|(i, _)| match search_type {
                    SearchType::Find => i,
                    SearchType::Till => i.saturating_sub(1).max(cursor_col + 1),
                })
        }
        SearchDirection::Backward => {
            if cursor_col == 0 {
                return None;
            }
            (0..cursor_col)
                .rev()
                .find(|&i| chars.get(i) == Some(&target))
                .map(|i| match search_type {
                    SearchType::Find => i,
                    SearchType::Till => (i + 1).min(cursor_col.saturating_sub(1)),
                })
        }
    }
}

#[cfg(test)]
#[path = "char_search_tests.rs"]
mod char_search_tests;
