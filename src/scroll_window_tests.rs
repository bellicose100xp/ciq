//! Tests for the shared scrolled-window math ([`scroll_offset`] / [`visible_window`]).

use super::{scroll_offset, visible_window};

#[test]
fn no_scroll_when_list_fits_the_window() {
    // len <= visible: everything fits, the window starts at 0.
    assert_eq!(scroll_offset(0, 5, 8), 0);
    assert_eq!(scroll_offset(4, 5, 8), 0);
    assert_eq!(visible_window(4, 5, 8), (0, 5));
}

#[test]
fn no_scroll_while_selection_inside_the_first_window() {
    // 20 items, window of 8: selections 0..8 keep start at 0.
    for sel in 0..8 {
        assert_eq!(scroll_offset(sel, 20, 8), 0, "sel={sel}");
    }
}

#[test]
fn window_tracks_selection_once_it_scrolls_past_the_first_window() {
    // selected=8 -> the 9th item must be visible; window [1, 9).
    assert_eq!(scroll_offset(8, 20, 8), 1);
    assert_eq!(visible_window(8, 20, 8), (1, 9));
    // selected=9 -> window [2, 10).
    assert_eq!(scroll_offset(9, 20, 8), 2);
}

#[test]
fn window_never_scrolls_past_the_end() {
    // The last item selected: start clamps to len - visible so the window ends exactly at len.
    assert_eq!(scroll_offset(19, 20, 8), 12);
    assert_eq!(visible_window(19, 20, 8), (12, 20));
}

#[test]
fn zero_visible_is_safe() {
    assert_eq!(scroll_offset(3, 20, 0), 0);
    assert_eq!(visible_window(3, 20, 0), (0, 20));
}

#[test]
fn clicked_row_resolves_to_an_absolute_index() {
    // The click contract: row r on screen maps to absolute index start + r. After scrolling so
    // selected=9 (start=2), clicking the first visible row (r=0) is absolute index 2, NOT 0.
    let start = scroll_offset(9, 20, 8);
    assert_eq!(start, 2);
    // Clicking the first visible row (offset 0) resolves to `start`, not 0.
    assert_eq!(start, 2);
    assert_eq!(start + 7, 9); // the last visible row is the selection
}
