//! The search bar's pure state machine + the any-column row filter.
//!
//! Mirrors the history popup's needle model (a plain `String`, pushed/popped per key — no
//! textarea) and jiq's visible/confirmed split: while **editing** (visible, not confirmed) typing
//! mutates the needle and the grid filters live; **confirming** (Enter) freezes the needle and
//! returns the keyboard to normal grid navigation with the filter still applied. Closing clears
//! everything — the full grid comes back.

use crate::engine::{Column, Table};

use super::matcher;

/// Search-bar state: visibility, the editing/confirmed mode, the needle, and the current match.
///
/// Every displayed (filtered) row is itself a match, so the "current match" is a row index into
/// the filtered table — the row `n`/`N` navigation lands on, painted with the distinct
/// current-match style. It defaults to the first row and resets there on every needle edit.
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    /// Whether the bar is on screen (editing OR confirmed).
    visible: bool,
    /// Whether Enter froze the needle — navigation keys route normally again while the filter
    /// stays applied. `Ctrl+F` reopens editing.
    confirmed: bool,
    /// The filter text, matched case-insensitively against every cell of every row.
    needle: String,
    /// The current-match row index into the *filtered* table (the row `n`/`N` land on and the
    /// render paints with [`crate::theme::grid::current_match`]). Reset to 0 on each needle edit;
    /// clamped by [`clamp_current`](Self::clamp_current) against the live filtered row count.
    current_row: usize,
}

impl SearchState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the bar in editing mode (a fresh open starts with the previous needle cleared by
    /// [`close`](Self::close), so typing starts clean).
    pub fn open(&mut self) {
        self.visible = true;
        self.confirmed = false;
    }

    /// Close the bar and clear the needle — the unfiltered grid is restored.
    pub fn close(&mut self) {
        self.visible = false;
        self.confirmed = false;
        self.needle.clear();
        self.current_row = 0;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn is_confirmed(&self) -> bool {
        self.confirmed
    }

    /// Whether the bar is visible and in editing mode (typing mutates the needle).
    pub fn is_editing(&self) -> bool {
        self.visible && !self.confirmed
    }

    /// Whether the grid should be filtered: the bar is visible with a non-empty needle.
    pub fn is_filtering(&self) -> bool {
        self.visible && !self.needle.is_empty()
    }

    /// Freeze the needle (Enter) — the filter stays, navigation resumes.
    pub fn confirm(&mut self) {
        self.confirmed = true;
    }

    /// Return to editing mode (`Ctrl+F` on a confirmed search).
    pub fn unconfirm(&mut self) {
        self.confirmed = false;
    }

    pub fn needle(&self) -> &str {
        &self.needle
    }

    /// Append a typed char to the needle. Resets the current match to the first row (the needle
    /// changed, so the old current-row index is meaningless).
    pub fn push(&mut self, c: char) {
        self.needle.push(c);
        self.current_row = 0;
    }

    /// Pop the last needle char (Backspace). Resets the current match to the first row.
    pub fn pop(&mut self) {
        self.needle.pop();
        self.current_row = 0;
    }

    /// The current-match row index into the filtered table.
    pub fn current_row(&self) -> usize {
        self.current_row
    }

    /// Move to the next match (`n` / Enter-when-confirmed), wrapping from the last back to the
    /// first. `filtered_count` is the live filtered row count; a count of 0 leaves the index at 0.
    pub fn next_match(&mut self, filtered_count: usize) {
        if filtered_count == 0 {
            self.current_row = 0;
            return;
        }
        self.current_row = (self.current_row + 1) % filtered_count;
    }

    /// Move to the previous match (`N`), wrapping from the first back to the last.
    pub fn prev_match(&mut self, filtered_count: usize) {
        if filtered_count == 0 {
            self.current_row = 0;
            return;
        }
        self.current_row = if self.current_row == 0 {
            filtered_count - 1
        } else {
            self.current_row - 1
        };
    }

    /// Clamp the current-match index into `[0, filtered_count)` (0 when empty). Called after the
    /// filter recomputes so a shrunk result never leaves `current_row` past the end.
    pub fn clamp_current(&mut self, filtered_count: usize) {
        self.current_row = self.current_row.min(filtered_count.saturating_sub(1));
    }
}

/// Whether row `r` of `table` matches `needle`: ANY column's displayed cell text contains it,
/// case-insensitively. A `NULL` cell displays as empty text, so it never matches a non-empty
/// needle (searching finds data, not absence).
pub fn row_matches(table: &Table, r: usize, needle: &str) -> bool {
    table
        .columns()
        .iter()
        .any(|col| matcher::contains(&col.cells[r].display(), needle))
}

/// The filtered projection of `table`: the rows where [`row_matches`], in original order, with
/// the same columns (names, types, order). Row order is inherited from the source table, so the
/// output is as deterministic as its input.
pub fn filter_table(table: &Table, needle: &str) -> Table {
    let keep: Vec<usize> = (0..table.row_count())
        .filter(|&r| row_matches(table, r, needle))
        .collect();
    Table::new(
        table
            .columns()
            .iter()
            .map(|col| {
                Column::new(
                    col.name.clone(),
                    col.ty.clone(),
                    keep.iter().map(|&r| col.cells[r].clone()).collect(),
                )
            })
            .collect(),
    )
}

#[cfg(test)]
#[path = "search_state_tests.rs"]
mod search_state_tests;
