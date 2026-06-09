//! The query-history ring state machine — pure owned data, pure transitions (`dev/PLAN.md` §7.6
//! P5.2). Ported from jiq's `history/history_state.rs`, with the JSON-filter entries replaced by
//! SQL query strings and jiq's `tui_textarea` search box replaced by a plain needle filtered with
//! the shared [`crate::text_match::is_subsequence`] (the same fuzzy rule the palette + autocomplete
//! ranker use).
//!
//! Two distinct navigation modes, both from jiq:
//!  - the **popup** (`open`/`close`/`select_next`/`select_previous`/`selected_entry`): a visible,
//!    fuzzy-filterable list with a cursor and a scrolled window;
//!  - **inline cycling** (`cycle_previous`/`cycle_next`): the Up-at-empty-bar shell-style walk
//!    through entries without opening the popup.
//!
//! Persistence is out of this file: the ring is pure and tested in-memory; the on-disk read/write
//! lives in [`storage`](super::storage) and is wired by the App. `add`/`recall`/dedupe/`navigate`/
//! `filter` are all `&mut self` / `&self` over plain `Vec<String>` and are unit-tested with plain
//! asserts — no terminal, no filesystem.

use crate::text_match::is_subsequence;

/// Max history rows shown in the popup at once (the visible window; a longer list scrolls with the
/// cursor). Mirrors jiq's `MAX_VISIBLE_HISTORY`.
pub const MAX_VISIBLE_HISTORY: usize = 15;

/// The built-in cap on the in-memory ring when no explicit `max_entries` was configured. Reuses the
/// `[history]` config default so the in-session ring and the on-disk file share one bound (mirrors
/// jiq's `MAX_HISTORY_ENTRIES`). A `Default`/`new` ring carries this so the in-session contract
/// ("the cap bounds the ring") holds even before the App threads its configured cap in via
/// [`HistoryState::with_max`].
pub const DEFAULT_MAX_ENTRIES: usize = crate::config::history_config::DEFAULT_MAX_ENTRIES;

/// The history ring: the entries (newest first), the fuzzy needle + its filtered view, the popup
/// cursor + scroll, and the inline-cycling index.
///
/// All fields private; transitions go through the methods so the invariants hold (the cursor stays
/// inside the filtered list; the entries stay newest-first, deduped, and capped to `max`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryState {
    /// Every entry, **newest first**. `add` inserts at the front; a re-added entry moves to front.
    entries: Vec<String>,
    /// Indices into `entries` (in `entries` order) matching the current needle. Recomputed on every
    /// needle edit + every `add`.
    filtered_indices: Vec<usize>,
    /// The fuzzy search needle (case-insensitive subsequence against entries). Empty -> all match.
    needle: String,
    /// The cursor into the **filtered** list (the highlighted popup row).
    selected_index: usize,
    /// The top of the visible popup window (scroll offset into the filtered list).
    scroll_offset: usize,
    /// Whether the popup is currently open.
    visible: bool,
    /// The inline-cycling index into `entries` (Up-at-empty-bar walk), independent of the popup
    /// cursor. `None` = not cycling.
    cycling_index: Option<usize>,
    /// The cap on `entries` — the in-session ring is bounded to this, matching the documented
    /// `[history] max_entries` contract (the on-disk file is bounded separately by storage). At
    /// least 1; defaults to [`DEFAULT_MAX_ENTRIES`] until the App sets the configured value via
    /// [`with_max`](Self::with_max).
    max: usize,
}

impl Default for HistoryState {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            filtered_indices: Vec::new(),
            needle: String::new(),
            selected_index: 0,
            scroll_offset: 0,
            visible: false,
            cycling_index: None,
            max: DEFAULT_MAX_ENTRIES,
        }
    }
}

impl HistoryState {
    /// An empty history (no entries) — the in-memory test constructor and the no-persistence
    /// default. The App seeds entries via [`load_entries`](Self::load_entries) from storage.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty history whose ring is capped to `max` entries (clamped to at least 1). The App
    /// builds the ring with the configured `[history] max_entries` so the in-session ring and the
    /// on-disk file share one bound.
    pub fn with_max(max: usize) -> Self {
        Self {
            max: max.max(1),
            ..Self::default()
        }
    }

    /// Re-cap the ring to `max` (clamped to at least 1), truncating any entries already past it.
    /// Called by the App when it learns the configured `[history] max_entries`.
    pub fn set_max(&mut self, max: usize) {
        self.max = max.max(1);
        self.trim_to_max();
    }

    /// The current ring cap (mostly for tests).
    pub fn max(&self) -> usize {
        self.max
    }

    /// Build a history pre-populated with `entries` (newest first), e.g. loaded from disk. Dedupes
    /// (keeping the first/newest occurrence) and drops blanks so a hand-edited file never injects a
    /// duplicate or empty recall target. Capped to [`DEFAULT_MAX_ENTRIES`]; the filtered view starts
    /// as "all".
    pub fn with_entries(entries: Vec<String>) -> Self {
        let mut s = Self::new();
        s.load_entries(entries);
        s
    }

    /// Build a history pre-populated with `entries` and capped to `max` (clamped to at least 1) —
    /// the App's startup path, seeding from the on-disk file with the configured cap so the
    /// in-session ring and the file share one bound.
    pub fn with_entries_max(entries: Vec<String>, max: usize) -> Self {
        let mut s = Self::with_max(max);
        s.load_entries(entries);
        s
    }

    /// Replace the ring with `entries` (newest first), deduped + blank-stripped + capped to `max`,
    /// and reset the filtered view to all. Used to seed from storage at startup.
    pub fn load_entries(&mut self, entries: Vec<String>) {
        let mut seen = std::collections::HashSet::new();
        self.entries = entries
            .into_iter()
            .filter(|e| !e.trim().is_empty())
            .filter(|e| seen.insert(e.clone()))
            .collect();
        self.trim_to_max();
        self.cycling_index = None;
        self.reset_filter();
    }

    // --- read-only accessors ---

    /// Every entry, newest first.
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// The current fuzzy needle.
    pub fn needle(&self) -> &str {
        &self.needle
    }

    /// Whether the popup is open.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Total entries in the ring (unfiltered).
    pub fn total_count(&self) -> usize {
        self.entries.len()
    }

    /// Entries matching the current needle.
    pub fn filtered_count(&self) -> usize {
        self.filtered_indices.len()
    }

    /// The popup cursor (index into the filtered list).
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// The popup scroll offset (top of the visible window).
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// The entry currently under the popup cursor, if the filtered list is non-empty — the recall
    /// target when the user presses Enter.
    pub fn selected_entry(&self) -> Option<&str> {
        self.filtered_indices
            .get(self.selected_index)
            .and_then(|&i| self.entries.get(i))
            .map(String::as_str)
    }

    /// The entry at filtered display index `i`, if any (newest-first display order; used by the
    /// renderer and mouse hit-testing).
    pub fn entry_at_display_index(&self, i: usize) -> Option<&str> {
        self.filtered_indices
            .get(i)
            .and_then(|&idx| self.entries.get(idx))
            .map(String::as_str)
    }

    /// The visible window of `(display_index, entry)` pairs for the popup — the
    /// [`MAX_VISIBLE_HISTORY`]-row slice starting at the scroll offset, in filtered (newest-first)
    /// order.
    pub fn visible_entries(&self) -> Vec<(usize, &str)> {
        self.filtered_indices
            .iter()
            .enumerate()
            .skip(self.scroll_offset)
            .take(MAX_VISIBLE_HISTORY)
            .filter_map(|(display_idx, &entry_idx)| {
                self.entries
                    .get(entry_idx)
                    .map(|e| (display_idx, e.as_str()))
            })
            .collect()
    }

    // --- add / dedupe ---

    /// Add `query` to the ring (the just-run query). Dedupes by **moving an existing identical
    /// entry to the front** (so the ring holds each query once, newest-first) — this also covers
    /// the "consecutive duplicate" case: re-running the same query as the last one is a no-op on
    /// ordering. A blank/whitespace-only query is ignored. Recomputes the filtered view so the
    /// popup reflects the new entry. Returns whether the ring changed.
    ///
    /// This is the in-memory half only; the App persists to disk separately
    /// ([`storage`](super::storage)).
    pub fn add(&mut self, query: &str) -> bool {
        let query = query.trim();
        if query.is_empty() {
            return false;
        }
        // Already newest? (the consecutive-duplicate fast path) — nothing changes.
        if self.entries.first().map(String::as_str) == Some(query) {
            return false;
        }
        self.entries.retain(|e| e != query);
        self.entries.insert(0, query.to_string());
        self.trim_to_max();
        self.cycling_index = None;
        self.update_filter();
        true
    }

    // --- popup open / close / navigate ---

    /// Open the popup, optionally seeding the needle with `initial_query` (jiq seeds it with the
    /// current bar text so the list pre-filters to similar queries). Resets the cursor + scroll to
    /// the top.
    pub fn open(&mut self, initial_query: Option<&str>) {
        self.visible = true;
        self.needle = initial_query.unwrap_or("").to_string();
        self.update_filter();
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Close the popup and clear the needle/cursor/scroll (the filtered view resets to all).
    pub fn close(&mut self) {
        self.visible = false;
        self.needle.clear();
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.reset_filter();
    }

    /// Move the popup cursor toward the next (older) entry, clamped at the end of the filtered list.
    /// (jiq's `select_next`.)
    pub fn select_next(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        if self.selected_index + 1 < self.filtered_indices.len() {
            self.selected_index += 1;
        }
        self.adjust_scroll_to_selection();
    }

    /// Move the popup cursor toward the previous (newer) entry, clamped at the top.
    pub fn select_previous(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        self.selected_index = self.selected_index.saturating_sub(1);
        self.adjust_scroll_to_selection();
    }

    /// Append a char to the needle, re-filter, and reset the cursor/scroll to the top of the new
    /// filtered list (jiq resets selection on every search change).
    pub fn push_needle(&mut self, c: char) {
        self.needle.push(c);
        self.on_needle_changed();
    }

    /// Remove the last char from the needle, re-filter, and reset the cursor/scroll.
    pub fn pop_needle(&mut self) {
        self.needle.pop();
        self.on_needle_changed();
    }

    /// Replace the whole needle, re-filter, and reset the cursor/scroll.
    pub fn set_needle(&mut self, needle: impl Into<String>) {
        self.needle = needle.into();
        self.on_needle_changed();
    }

    // --- inline cycling (Up-at-empty-bar shell walk) ---

    /// Step to the previous (older) entry in the inline cycle, returning it to drop into the bar.
    /// First call returns the newest entry; subsequent calls walk older, stopping at the oldest.
    /// `None` when the ring is empty. (jiq's `cycle_previous`.)
    pub fn cycle_previous(&mut self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let next = match self.cycling_index {
            None => 0,
            Some(i) if i + 1 < self.entries.len() => i + 1,
            Some(i) => i, // at the oldest, stay
        };
        self.cycling_index = Some(next);
        self.entries.get(next).cloned()
    }

    /// Step toward the newer end of the inline cycle. At the newest entry it resets cycling and
    /// returns `None` (so the bar can restore the user's draft). `None` when not cycling.
    pub fn cycle_next(&mut self) -> Option<String> {
        match self.cycling_index {
            None => None,
            Some(0) => {
                self.cycling_index = None;
                None
            }
            Some(i) => {
                self.cycling_index = Some(i - 1);
                self.entries.get(i - 1).cloned()
            }
        }
    }

    /// Reset the inline-cycling walk (e.g. when the user edits the bar). The next `cycle_previous`
    /// starts fresh from the newest entry.
    pub fn reset_cycling(&mut self) {
        self.cycling_index = None;
    }

    /// The current inline-cycling index, if cycling (mostly for tests).
    pub fn cycling_index(&self) -> Option<usize> {
        self.cycling_index
    }

    // --- internals ---

    /// Drop the oldest entries past the `max` cap (entries are newest-first, so truncating from the
    /// end discards the oldest). The on-disk file is bounded separately by the storage layer; this
    /// keeps the in-session ring to the same documented `max_entries` bound.
    fn trim_to_max(&mut self) {
        self.entries.truncate(self.max);
    }

    /// Re-filter, then reset cursor + scroll to the top (used on every needle edit).
    fn on_needle_changed(&mut self) {
        self.update_filter();
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Reset the filtered view to "all entries", cursor + scroll at the top.
    fn reset_filter(&mut self) {
        self.filtered_indices = (0..self.entries.len()).collect();
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Recompute `filtered_indices` from the needle (case-insensitive subsequence), preserving
    /// `entries` order (newest-first). An empty needle matches everything. Clamps the cursor back
    /// into the (possibly shorter) list.
    fn update_filter(&mut self) {
        if self.needle.is_empty() {
            self.filtered_indices = (0..self.entries.len()).collect();
        } else {
            let needle = self.needle.to_ascii_lowercase();
            self.filtered_indices = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| is_subsequence(&e.to_ascii_lowercase(), &needle))
                .map(|(i, _)| i)
                .collect();
        }
        if self.selected_index >= self.filtered_indices.len() {
            self.selected_index = self.filtered_indices.len().saturating_sub(1);
        }
        self.adjust_scroll_to_selection();
    }

    /// Keep the cursor inside the visible window with the jiq-style SCROLLOFF margin: the cursor
    /// stays ~4 rows from the top/bottom of the popup before the window slides (clamped to half
    /// the viewport on tiny windows). Delegates to the shared
    /// [`crate::scroll_window::scroll_offset_for_cursor`] so every list popup follows one rule.
    fn adjust_scroll_to_selection(&mut self) {
        self.scroll_offset = crate::scroll_window::scroll_offset_for_cursor(
            self.selected_index,
            self.filtered_indices.len(),
            MAX_VISIBLE_HISTORY,
            self.scroll_offset,
        );
    }
}

#[cfg(test)]
#[path = "history_state_tests.rs"]
mod history_state_tests;
