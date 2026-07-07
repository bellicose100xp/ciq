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
            // Confirming an empty needle closes instead — there is nothing to freeze.
            Key::Enter | Key::Tab => {
                if self.search.needle().is_empty() {
                    self.close_search();
                } else {
                    self.search.confirm();
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
        // The old offset indexes a different row set; snap back to the top of the new one.
        self.v_row_offset = 0;
    }
}
