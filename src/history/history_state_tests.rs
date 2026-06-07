//! Tests for the pure history ring (`history_state.rs`) — add/dedupe/recall/navigate/filter over
//! an in-memory store. No filesystem, no terminal.

use super::{DEFAULT_MAX_ENTRIES, HistoryState, MAX_VISIBLE_HISTORY};

// --- add + dedupe (newest-first, unique) ---

#[test]
fn new_is_empty() {
    let h = HistoryState::new();
    assert_eq!(h.total_count(), 0);
    assert_eq!(h.filtered_count(), 0);
    assert_eq!(h.selected_entry(), None);
}

#[test]
fn add_inserts_newest_first() {
    let mut h = HistoryState::new();
    assert!(h.add("SELECT 1"));
    assert!(h.add("SELECT 2"));
    assert_eq!(
        h.entries(),
        &["SELECT 2".to_string(), "SELECT 1".to_string()]
    );
}

#[test]
fn add_blank_is_ignored() {
    let mut h = HistoryState::new();
    assert!(!h.add("   "));
    assert!(!h.add(""));
    assert_eq!(h.total_count(), 0);
}

#[test]
fn add_consecutive_duplicate_is_noop() {
    let mut h = HistoryState::new();
    assert!(h.add("SELECT 1"));
    // Re-running the same query (the consecutive-duplicate case) doesn't reorder or grow.
    assert!(!h.add("SELECT 1"));
    assert_eq!(h.total_count(), 1);
}

#[test]
fn add_non_consecutive_duplicate_moves_to_front() {
    let mut h = HistoryState::new();
    h.add("a");
    h.add("b");
    h.add("a"); // re-run an older query: it moves to front, count stays 2 (deduped)
    assert_eq!(h.entries(), &["a".to_string(), "b".to_string()]);
    assert_eq!(h.total_count(), 2);
}

#[test]
fn add_trims_whitespace() {
    let mut h = HistoryState::new();
    h.add("  SELECT 1  ");
    assert_eq!(h.entries(), &["SELECT 1".to_string()]);
}

// --- the in-session ring is capped to `max` (the documented max_entries contract) ---

#[test]
fn default_ring_carries_the_built_in_cap() {
    assert_eq!(HistoryState::new().max(), DEFAULT_MAX_ENTRIES);
}

#[test]
fn add_beyond_the_cap_drops_the_oldest_entry() {
    let mut h = HistoryState::with_max(3);
    for q in ["a", "b", "c", "d"] {
        h.add(q);
    }
    // Newest-first, capped to 3: "a" (the oldest) was dropped.
    assert_eq!(h.total_count(), 3);
    assert_eq!(
        h.entries(),
        &["d".to_string(), "c".to_string(), "b".to_string()]
    );
}

#[test]
fn with_max_clamps_zero_to_one() {
    let mut h = HistoryState::with_max(0);
    assert_eq!(h.max(), 1);
    h.add("a");
    h.add("b");
    assert_eq!(
        h.entries(),
        &["b".to_string()],
        "cap of 1 keeps only newest"
    );
}

#[test]
fn with_entries_max_caps_seeded_entries() {
    // A long on-disk file seeds only `max` entries into the ring (newest-first preserved).
    let entries: Vec<String> = (0..10).map(|i| format!("q{i}")).collect();
    let h = HistoryState::with_entries_max(entries, 4);
    assert_eq!(h.total_count(), 4);
    assert_eq!(
        h.entries(),
        &[
            "q0".to_string(),
            "q1".to_string(),
            "q2".to_string(),
            "q3".to_string()
        ]
    );
}

#[test]
fn set_max_shrinks_an_existing_ring() {
    let mut h = HistoryState::new();
    for i in 0..5 {
        h.add(&format!("q{i}"));
    }
    assert_eq!(h.total_count(), 5);
    h.set_max(2);
    assert_eq!(h.total_count(), 2);
    // Newest two survive (entries are newest-first: q4, q3).
    assert_eq!(h.entries(), &["q4".to_string(), "q3".to_string()]);
}

// --- with_entries / load_entries: dedupe + blank-strip ---

#[test]
fn with_entries_dedupes_and_strips_blanks() {
    let h = HistoryState::with_entries(vec![
        "a".into(),
        "".into(),
        "b".into(),
        "a".into(), // dup
        "   ".into(),
    ]);
    assert_eq!(h.entries(), &["a".to_string(), "b".to_string()]);
}

#[test]
fn load_entries_resets_cycling_and_filter() {
    let mut h = HistoryState::new();
    h.add("old");
    h.cycle_previous();
    h.load_entries(vec!["x".into(), "y".into()]);
    assert_eq!(h.cycling_index(), None);
    assert_eq!(h.filtered_count(), 2);
}

// --- popup open / close ---

#[test]
fn open_sets_visible_and_seeds_needle() {
    let mut h = HistoryState::with_entries(vec!["select x".into(), "select y".into()]);
    h.open(Some("select"));
    assert!(h.is_visible());
    assert_eq!(h.needle(), "select");
    assert_eq!(h.filtered_count(), 2);
    assert_eq!(h.selected_index(), 0);
}

#[test]
fn open_with_none_seeds_empty_needle() {
    let mut h = HistoryState::with_entries(vec!["a".into()]);
    h.open(None);
    assert_eq!(h.needle(), "");
}

#[test]
fn close_clears_needle_and_resets() {
    let mut h = HistoryState::with_entries(vec!["a".into(), "b".into()]);
    h.open(Some("a"));
    h.close();
    assert!(!h.is_visible());
    assert_eq!(h.needle(), "");
    assert_eq!(h.filtered_count(), 2); // filter reset to all
    assert_eq!(h.selected_index(), 0);
}

// --- navigate (cursor + scroll) ---

#[test]
fn select_next_and_previous_walk_and_clamp() {
    let mut h = HistoryState::with_entries(vec!["a".into(), "b".into(), "c".into()]);
    h.open(None);
    assert_eq!(h.selected_entry(), Some("a")); // newest first
    h.select_next();
    assert_eq!(h.selected_entry(), Some("b"));
    h.select_next();
    assert_eq!(h.selected_entry(), Some("c"));
    h.select_next(); // clamp at end
    assert_eq!(h.selected_entry(), Some("c"));
    h.select_previous();
    assert_eq!(h.selected_entry(), Some("b"));
    h.select_previous();
    h.select_previous(); // clamp at top
    assert_eq!(h.selected_entry(), Some("a"));
}

#[test]
fn select_on_empty_is_noop() {
    let mut h = HistoryState::new();
    h.open(None);
    h.select_next();
    h.select_previous();
    assert_eq!(h.selected_entry(), None);
}

#[test]
fn scroll_follows_cursor_past_window() {
    // More entries than the window: walking the cursor down scrolls the window.
    let entries: Vec<String> = (0..MAX_VISIBLE_HISTORY + 5)
        .map(|i| format!("q{i}"))
        .collect();
    let mut h = HistoryState::with_entries(entries);
    h.open(None);
    assert_eq!(h.scroll_offset(), 0);
    for _ in 0..MAX_VISIBLE_HISTORY {
        h.select_next();
    }
    // Cursor at index MAX_VISIBLE_HISTORY -> window scrolled by 1.
    assert!(h.scroll_offset() >= 1);
    assert_eq!(h.selected_index(), MAX_VISIBLE_HISTORY);
}

#[test]
fn visible_entries_capped_to_window() {
    let entries: Vec<String> = (0..MAX_VISIBLE_HISTORY + 10)
        .map(|i| format!("q{i}"))
        .collect();
    let mut h = HistoryState::with_entries(entries);
    h.open(None);
    assert_eq!(h.visible_entries().len(), MAX_VISIBLE_HISTORY);
}

// --- fuzzy filter (search) ---

#[test]
fn needle_filters_by_subsequence_case_insensitive() {
    let mut h = HistoryState::with_entries(vec![
        "SELECT id FROM t".into(),
        "SELECT name FROM t".into(),
        "DELETE noise".into(),
    ]);
    h.open(None);
    h.set_needle("select");
    assert_eq!(h.filtered_count(), 2);
    // A subsequence match, not substring: "snt" matches "SELECT name FROM t".
    h.set_needle("snt");
    assert_eq!(h.filtered_count(), 1);
    assert_eq!(h.selected_entry(), Some("SELECT name FROM t"));
}

#[test]
fn push_pop_needle_refilters_and_resets_cursor() {
    let mut h = HistoryState::with_entries(vec!["abc".into(), "axc".into(), "zzz".into()]);
    h.open(None);
    h.select_next(); // move cursor off the top
    h.push_needle('a');
    assert_eq!(h.selected_index(), 0); // reset on filter change
    assert_eq!(h.filtered_count(), 2); // abc, axc
    h.push_needle('b');
    assert_eq!(h.filtered_count(), 1); // ab -> abc
    h.pop_needle();
    assert_eq!(h.filtered_count(), 2);
}

#[test]
fn empty_filtered_list_has_no_selection() {
    let mut h = HistoryState::with_entries(vec!["abc".into()]);
    h.open(None);
    h.set_needle("zzz"); // no match
    assert_eq!(h.filtered_count(), 0);
    assert_eq!(h.selected_entry(), None);
}

#[test]
fn entry_at_display_index() {
    let mut h = HistoryState::with_entries(vec!["a".into(), "b".into()]);
    h.open(None);
    assert_eq!(h.entry_at_display_index(0), Some("a"));
    assert_eq!(h.entry_at_display_index(1), Some("b"));
    assert_eq!(h.entry_at_display_index(2), None);
}

// --- inline cycling (Up-at-empty-bar walk) ---

#[test]
fn cycle_previous_walks_older_and_stops() {
    let mut h = HistoryState::with_entries(vec!["new".into(), "mid".into(), "old".into()]);
    assert_eq!(h.cycle_previous().as_deref(), Some("new"));
    assert_eq!(h.cycle_previous().as_deref(), Some("mid"));
    assert_eq!(h.cycle_previous().as_deref(), Some("old"));
    assert_eq!(h.cycle_previous().as_deref(), Some("old")); // stays at oldest
}

#[test]
fn cycle_next_walks_newer_then_resets() {
    let mut h = HistoryState::with_entries(vec!["new".into(), "old".into()]);
    h.cycle_previous(); // -> new
    h.cycle_previous(); // -> old
    assert_eq!(h.cycle_next().as_deref(), Some("new"));
    assert_eq!(h.cycle_next(), None); // at newest -> reset
    assert_eq!(h.cycling_index(), None);
}

#[test]
fn cycle_next_without_cycling_is_none() {
    let mut h = HistoryState::with_entries(vec!["a".into()]);
    assert_eq!(h.cycle_next(), None);
}

#[test]
fn cycle_on_empty_is_none() {
    let mut h = HistoryState::new();
    assert_eq!(h.cycle_previous(), None);
}

#[test]
fn reset_cycling_restarts_walk() {
    let mut h = HistoryState::with_entries(vec!["a".into(), "b".into()]);
    h.cycle_previous(); // -> a
    h.reset_cycling();
    assert_eq!(h.cycling_index(), None);
    assert_eq!(h.cycle_previous().as_deref(), Some("a")); // fresh from newest
}

#[test]
fn add_resets_cycling() {
    let mut h = HistoryState::with_entries(vec!["a".into()]);
    h.cycle_previous();
    h.add("b");
    assert_eq!(h.cycling_index(), None);
}
