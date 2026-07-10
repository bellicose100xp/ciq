//! Search-bar orchestration (`Ctrl+F` row filter) — an `impl App` block lifted out of `app.rs`
//! (like `palette_app` / `mouse_app`) to keep that file under the line cap.
//!
//! The App owns a [`SearchState`](crate::search::SearchState) plus a cached filtered projection
//! of the current result. The filter is recomputed on every needle edit and every new result
//! (never per frame), and everything display-shaped — grid layout, scroll bounds, hover
//! hit-tests, the row counter, the empty state — reads [`App::display_rows`] so the filtered
//! and unfiltered worlds can never disagree.

use crate::app::{App, Focus, Key, KeyEvent};
use crate::engine::Table;
use crate::search::search_state::filter_table;

impl App {
    /// The table the grid displays: the search-filtered projection while the `Ctrl+F` filter is
    /// active, else the full result. Every display consumer (render, scroll clamps, mouse
    /// hover, row counter, empty state) reads this — not `result.rows` — so the filter applies
    /// everywhere at once.
    pub fn display_rows(&self) -> Option<&Table> {
        if self.search.is_filtering()
            && let Some(filtered) = self.search_filtered.as_ref()
        {
            return Some(filtered);
        }
        self.result.as_ref().map(|r| &r.rows)
    }

    pub fn search(&self) -> &crate::search::SearchState {
        &self.search
    }

    /// Open the search bar (`Ctrl+F`). On a *confirmed* search the same chord re-enters editing
    /// instead (jiq's unconfirm), keeping the needle. Focus moves to the results pane so a
    /// confirm lands directly in grid navigation. A no-op before the first result — there are
    /// no rows to filter.
    pub(crate) fn open_search(&mut self) {
        if self.result.is_none() {
            return;
        }
        self.close_facet();
        if self.search.is_confirmed() {
            self.search.unconfirm();
            return;
        }
        if !self.search.is_visible() {
            self.search.open();
            self.focus = Focus::Results;
        }
    }

    /// Close the bar and drop the filter — the full grid comes back, scroll reset to the top
    /// (the filtered offset is meaningless against the unfiltered row set).
    pub(crate) fn close_search(&mut self) {
        self.search.close();
        self.search_filtered = None;
        self.v_row_offset = 0;
    }

    /// Recompute the cached filtered projection from the current result + needle. Called on
    /// every needle edit and every new result; `None` whenever nothing is being filtered.
    pub(crate) fn refresh_search_filter(&mut self) {
        self.search_filtered = match (self.search.is_filtering(), self.result.as_ref()) {
            (true, Some(result)) => Some(filter_table(&result.rows, self.search.needle())),
            _ => None,
        };
        // A shrunk filtered set must not leave the current-match index past the end.
        let count = self
            .search_filtered
            .as_ref()
            .map(|t| t.row_count())
            .unwrap_or(0);
        self.search.clamp_current(count);
    }

    /// Route a key while the search bar is in **editing** mode (it captures the keyboard, like
    /// the other popups): typing edits the needle and re-filters live, Enter/Tab confirm (freeze
    /// the filter, resume grid navigation), Esc closes and clears, Ctrl-C still quits. Returns
    /// `true` if the app should quit.
    pub(crate) fn handle_search_key(&mut self, ev: &KeyEvent) -> bool {
        if ev.is_quit() {
            return true;
        }
        match ev.key {
            Key::Esc => self.close_search(),
            // Confirming an empty needle closes instead — there is nothing to freeze. Confirming
            // a non-empty search freezes it and scrolls the (first) current match into view.
            Key::Enter | Key::Tab => {
                if self.search.needle().is_empty() {
                    self.close_search();
                } else {
                    self.search.confirm();
                    self.scroll_current_match_into_view();
                }
            }
            Key::Backspace => {
                self.search.pop();
                self.on_search_needle_changed();
            }
            Key::Char(c) if !ev.mods.ctrl && !ev.mods.alt => {
                self.search.push(c);
                self.on_search_needle_changed();
            }
            _ => {}
        }
        false
    }

    fn on_search_needle_changed(&mut self) {
        self.refresh_search_filter();
        // The old offset indexes a different row set; reset scroll to the origin, then scroll to
        // the (reset-to-first) current match — jiq highlights AND scrolls to the first match live
        // as you type, not only after confirming. `current_row` is already 0 (push/pop reset it),
        // so this brings the first match into view with the scrolloff margin.
        self.v_row_offset = 0;
        self.h_col_offset = 0;
        self.h_char_offset = 0;
        self.scroll_current_match_into_view();
    }

    /// Step to the next matching row (`n`, or Enter on a confirmed search) and scroll it into
    /// view. No-op unless a confirmed search is filtering rows.
    pub(crate) fn search_next_match(&mut self) {
        let count = self.display_rows().map(|r| r.row_count()).unwrap_or(0);
        self.search.next_match(count);
        self.scroll_current_match_into_view();
    }

    /// Step to the previous matching row (`N`) and scroll it into view.
    pub(crate) fn search_prev_match(&mut self) {
        let count = self.display_rows().map(|r| r.row_count()).unwrap_or(0);
        self.search.prev_match(count);
        self.scroll_current_match_into_view();
    }

    /// Scroll the grid so the current-match row (and, for the leftmost matching cell, its column)
    /// is comfortably inside the viewport — never flush against a pane edge unless it is the very
    /// first/last row or column. Vim's `scrolloff`, applied on both axes (jiq parity).
    pub(crate) fn scroll_current_match_into_view(&mut self) {
        let Some(rows) = self.display_rows() else {
            return;
        };
        let row_count = rows.row_count();
        if row_count == 0 {
            return;
        }
        let cur = self.search.current_row().min(row_count.saturating_sub(1));

        // Vertical: keep `cur` inside the SCROLLOFF margin of the visible body window.
        let body_h = self.results_body_height() as usize;
        if body_h > 0 {
            self.v_row_offset = crate::scroll_window::scroll_offset_for_cursor(
                cur,
                row_count,
                body_h,
                self.v_row_offset,
            );
        }

        // Horizontal: bring the leftmost column whose cell matches the needle into view, with a
        // one-column margin so it isn't flush to the left/right border. Falls back to no h-scroll
        // change when no specific column matches (e.g. the match is only in an off-screen wide
        // cell — the vertical scroll already surfaced the row).
        if let Some(col) = self.current_match_column() {
            self.scroll_column_into_view(col);
        }
    }

    /// The index of the leftmost column whose current-match-row cell contains the needle, if any.
    fn current_match_column(&self) -> Option<usize> {
        let needle = self.search.needle();
        if needle.is_empty() {
            return None;
        }
        let rows = self.display_rows()?;
        let cur = self
            .search
            .current_row()
            .min(rows.row_count().saturating_sub(1));
        rows.columns()
            .iter()
            .position(|col| crate::search::matcher::contains(&col.cells[cur].display(), needle))
    }

    /// Slide `h_col_offset` (and its char twin) so column `col` sits inside the viewport with a
    /// one-column scrolloff margin from whichever edge it was off — left OR right. Width-aware via
    /// [`h_col_offset_to_reveal`](crate::grid::grid_layout::h_col_offset_to_reveal), so an
    /// off-screen-right column genuinely scrolls the grid rightward.
    fn scroll_column_into_view(&mut self, col: usize) {
        let Some(rows) = self.display_rows() else {
            return;
        };
        if rows.col_count() == 0 {
            return;
        }
        const H_MARGIN: usize = 1;
        let inner_w = self.results_inner_width();
        let widths = crate::grid::col_width::interactive_widths(rows, inner_w);
        self.h_col_offset = crate::grid::grid_layout::h_col_offset_to_reveal(
            &widths,
            inner_w,
            col,
            self.h_col_offset,
            H_MARGIN,
        );
        self.snap_h_char_to_col();
    }
}
