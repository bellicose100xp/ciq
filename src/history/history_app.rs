//! Query-history App orchestration (`dev/PLAN.md` §7.6 P5.2) — an `impl App` block lifted out of
//! `app.rs` to keep that file under the 1000-line cap, and because it is cohesive: open/close the
//! popup, route its keys, recall a selected entry through the normal dispatch path, and record a
//! dispatched query into the ring (persisting it when enabled).
//!
//! It reuses the App's own `schedule` / `refresh_autocomplete` (the same debounce + preprocess +
//! dispatch path a typed query takes) — a recalled query is **not** a privileged bypass; it hits
//! the read-only single-statement guard like any other. All of this is headless: synthetic
//! `KeyEvent`s in, state out; the on-disk persistence is the storage seam (tempdir-tested).

use crate::app::{App, AppPhase, KeyEvent};
use crate::history::history_events::{HistoryAction, map_key as map_history_key};
use crate::history::storage as history_storage;

impl App {
    /// Open the history popup (P5.2). No-op while loading / on a load error. Seeds the fuzzy needle
    /// with the current bar text so the list pre-filters to similar prior queries (jiq's behavior).
    /// Closes the other popups so overlays never stack.
    pub(crate) fn open_history(&mut self) {
        if matches!(self.phase, AppPhase::Loading | AppPhase::LoadError(_)) {
            return;
        }
        self.autocomplete.close();
        self.palette_open = false;
        // Seed the needle with whatever the dispatcher would send right now (the composed SQL in
        // Simple mode, the textarea in Power) so a list pre-filters to "queries that look like the
        // current one". `query()` is the same string the dispatch path sees.
        let seed = self.query();
        self.history.open(Some(&seed));
        self.history_open = true;
    }

    /// Close the history popup without recalling.
    pub(crate) fn close_history(&mut self) {
        self.history.close();
        self.history_open = false;
    }

    /// Handle a key while the history popup is open (P5.2). Returns whether the app should quit
    /// (only `Ctrl+C` quits). The chord set is the pure [`map_history_key`] mapping:
    ///  - `Up`/`Down` move the cursor through the filtered list;
    ///  - a printable char appends to the fuzzy needle; `Backspace` pops it;
    ///  - `Enter`/`Tab` recall the selected entry into the bar (-> the normal dispatch path) and
    ///    close;
    ///  - `Esc` closes without recalling.
    pub(crate) fn handle_history_key(&mut self, ev: &KeyEvent, now_ms: u64) -> bool {
        match map_history_key(ev) {
            HistoryAction::Quit => return true,
            HistoryAction::SelectNext => self.history.select_next(),
            HistoryAction::SelectPrevious => self.history.select_previous(),
            HistoryAction::Push(c) => self.history.push_needle(c),
            HistoryAction::Pop => self.history.pop_needle(),
            HistoryAction::Accept => self.recall_selected_history(now_ms),
            HistoryAction::Close => self.close_history(),
            HistoryAction::Ignore => {}
        }
        false
    }

    /// Recall the selected history entry into the query bar and close the popup. The recalled SQL
    /// is dropped into the bar verbatim and scheduled through the **same** debounce + preprocess-
    /// validate + dispatch path a typed query uses (§7.6) — so it goes through the read-only
    /// single-statement guard like any other query, never a privileged path. No-op (just closes)
    /// when nothing is selected.
    pub(crate) fn recall_selected_history(&mut self, now_ms: u64) {
        let recalled = self.history.selected_entry().map(str::to_string);
        if let Some(sql) = recalled {
            // Recalled SQL is a full string. In Simple mode try to parse it back into the five
            // panes; on failure (the entry has features Simple can't represent) fall through to
            // Power mode with a status message — the user explicitly recalled this entry, so
            // pushing them to Power is correct (they can refine and re-toggle when ready).
            use crate::app::QueryMode;
            use crate::app::query_form::try_simplify_from_sql;
            let limit = self.viewport_row_limit();
            match self.query_form.mode() {
                QueryMode::Simple => match try_simplify_from_sql(&sql) {
                    Ok(parts) => self.query_form.enter_simple_with_parts(parts, limit),
                    Err(e) => {
                        self.query_form.enter_power_with_sql(&sql);
                        self.set_status(format!("can't simplify history entry: {}", e.message()));
                    }
                },
                QueryMode::Power => self.query_form.power_mut().set_text(&sql),
            }
            self.refresh_autocomplete();
            self.schedule(now_ms);
        }
        self.close_history();
    }

    /// Record `query` in the history ring (in-memory, newest-first, deduped) and persist it to disk
    /// when enabled. Called when a query is successfully dispatched (the felt "I ran this" moment),
    /// so only real, accepted queries enter history. A blank query is ignored by the ring.
    pub(crate) fn record_history(&mut self, query: &str) {
        if !self.history.add(query) {
            return; // blank or already-newest: nothing to persist either
        }
        if self.history_persist
            && let Some(path) = self.history_path.as_ref()
            && let Err(e) = history_storage::add(path, query, self.history_max)
        {
            log::warn!("failed to persist history entry: {e}");
        }
    }
}
