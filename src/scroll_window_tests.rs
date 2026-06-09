//! Tests for the shared scrolled-window math ([`scroll_offset`] / [`visible_window`] /
//! [`scroll_offset_for_cursor`]) — the jiq-style SCROLLOFF margin every list popup shares.

use super::{SCROLLOFF, scroll_offset, scroll_offset_for_cursor, visible_window};

// ── `scroll_offset` (stateless: smallest offset that keeps the cursor inside the margin) ─────────

#[test]
fn no_scroll_when_list_fits_the_window() {
    // len <= visible: everything fits, the window starts at 0.
    assert_eq!(scroll_offset(0, 5, 8), 0);
    assert_eq!(scroll_offset(4, 5, 8), 0);
    assert_eq!(visible_window(4, 5, 8), (0, 5));
}

#[test]
fn no_scroll_while_selection_inside_the_top_margin() {
    // 20 items, viewport 8, SCROLLOFF=4: selections 0..=3 are within the upper band so the
    // smallest valid offset is 0 (cursor is at most 3 rows from the data top, within margin).
    for sel in 0..=3 {
        assert_eq!(scroll_offset(sel, 20, 8), 0, "sel={sel}");
    }
}

#[test]
fn window_slides_with_the_selection_once_past_the_margin() {
    // viewport 8, SCROLLOFF=4 -> stateless rule: offset = selected - SCROLLOFF, clamped.
    //   selected=4 -> offset = 0 (margin satisfied, no slide yet)
    //   selected=5 -> offset = 1
    //   selected=8 -> offset = 4 (cursor sits SCROLLOFF rows from the top of the window)
    assert_eq!(scroll_offset(4, 20, 8), 0);
    assert_eq!(scroll_offset(5, 20, 8), 1);
    assert_eq!(scroll_offset(8, 20, 8), 4);
    assert_eq!(visible_window(8, 20, 8), (4, 12));
}

#[test]
fn window_never_scrolls_past_the_end() {
    // The last item selected: offset clamps to `len - visible` so the window ends at len —
    // the cursor sits at the bottom edge because there's no more data below the margin.
    assert_eq!(scroll_offset(19, 20, 8), 12);
    assert_eq!(visible_window(19, 20, 8), (12, 20));
}

#[test]
fn tiny_viewport_clamps_scrolloff_to_half_the_viewport() {
    // viewport=3 -> effective scrolloff = 1. Margin shrinks; the middle row tracks the cursor.
    assert_eq!(scroll_offset(0, 10, 3), 0);
    assert_eq!(scroll_offset(1, 10, 3), 0);
    assert_eq!(scroll_offset(2, 10, 3), 1);
    assert_eq!(scroll_offset(5, 10, 3), 4);
    assert_eq!(scroll_offset(9, 10, 3), 7);

    // viewport=2 -> effective = 1; viewport=1 -> effective = 0. Both safe (no panic).
    assert_eq!(scroll_offset(0, 10, 2), 0);
    assert_eq!(scroll_offset(9, 10, 2), 8);
    assert_eq!(scroll_offset(0, 10, 1), 0);
    assert_eq!(scroll_offset(5, 10, 1), 5);
    assert_eq!(scroll_offset(9, 10, 1), 9);
}

#[test]
fn zero_visible_or_zero_len_is_safe() {
    assert_eq!(scroll_offset(3, 20, 0), 0);
    assert_eq!(scroll_offset(0, 0, 8), 0);
    assert_eq!(visible_window(3, 20, 0), (0, 20));
    assert_eq!(visible_window(0, 0, 8), (0, 0));
}

#[test]
fn clicked_row_resolves_to_an_absolute_index() {
    // The click contract: row r on screen maps to absolute index start + r. After scrolling so
    // selected=8 (with SCROLLOFF=4 -> start=4), clicking the first visible row (r=0) is absolute
    // index 4, NOT 0; the cursor sits SCROLLOFF rows from the top of the window.
    let start = scroll_offset(8, 20, 8);
    assert_eq!(start, 4);
    assert_eq!(8 - start, SCROLLOFF);
}

// ── `scroll_offset_for_cursor` (stateful: only slides when the cursor crosses a margin band) ─────

#[test]
fn stateful_no_change_when_cursor_inside_the_body() {
    // viewport 10, SCROLLOFF=4 -> body band starts at offset+4 and ends at offset+(10-4)=6.
    // With offset=5 and selected=9 (window row 4), cursor is at the lower margin boundary (not
    // past it), so the offset doesn't change.
    let off = scroll_offset_for_cursor(9, 30, 10, 5);
    assert_eq!(off, 5);
}

#[test]
fn stateful_walks_down_one_row_at_a_time_into_the_lower_band() {
    // viewport 8, SCROLLOFF=4. Walk select=0 -> 19 driven by call (current_offset threaded
    // forward). Once the cursor crosses the lower band threshold the offset slides one row at a
    // time so the cursor stays SCROLLOFF rows from the bottom of the visible window.
    let mut off = 0usize;
    let trail: Vec<(usize, usize)> = (0..20)
        .map(|sel| {
            off = scroll_offset_for_cursor(sel, 20, 8, off);
            (sel, off)
        })
        .collect();

    // Within the upper window the offset stays at 0 — until the cursor crosses the lower
    // threshold (offset + visible - SCROLLOFF = 0 + 8 - 4 = 4). At sel=4 it slides:
    //   sel=0..=3 -> off=0  (cursor not yet past the lower threshold of 4)
    assert_eq!(trail[0], (0, 0));
    assert_eq!(trail[3], (3, 0));
    // selected=4 -> selected >= 4, slide; new_offset = 4 + 4 + 1 - 8 = 1.
    assert_eq!(trail[4], (4, 1));
    assert_eq!(trail[5], (5, 2));
    assert_eq!(trail[6], (6, 3));
    assert_eq!(trail[12], (12, 9));
    assert_eq!(trail[15], (15, 12)); // max offset = 20 - 8 = 12
    assert_eq!(
        trail[19],
        (19, 12),
        "max offset clamps so the last window ends exactly at len"
    );
}

#[test]
fn stateful_walks_up_into_the_upper_band() {
    // Walk in reverse from sel=19 with offset bootstrapped to 12, viewport 8, SCROLLOFF=4.
    let mut off = scroll_offset_for_cursor(19, 20, 8, 0);
    assert_eq!(off, 12); // bootstrap: jumped past the lower threshold so it lands at max.
    let trail: Vec<(usize, usize)> = (0..=19)
        .rev()
        .map(|sel| {
            off = scroll_offset_for_cursor(sel, 20, 8, off);
            (sel, off)
        })
        .collect();
    let pair_at = |sel: usize| trail.iter().find(|(s, _)| *s == sel).copied().unwrap();

    // viewport=8, SCROLLOFF=4 collapses the body band to zero width (visible - 2*SCROLLOFF = 0),
    // so the cursor never sits "in the middle" — every selection move triggers a slide. Walking
    // backward from (sel=19, off=12) the offset stays at 12 while sel >= 16 (the lower threshold),
    // then slides up one row at a time once the cursor crosses into the upper band:
    //   sel=19..=16 -> off stays 12 (cursor in lower margin, slide-down clamped to max)
    //   sel=15:      off slides up to max(0, 15 - 4) = 11
    //   sel=14..=5:  off = sel - 4
    //   sel=4..=0:   off clamps to 0 (data top)
    assert_eq!(pair_at(19), (19, 12));
    assert_eq!(pair_at(16), (16, 12));
    assert_eq!(pair_at(15), (15, 11));
    assert_eq!(pair_at(14), (14, 10));
    assert_eq!(pair_at(13), (13, 9));
    assert_eq!(pair_at(5), (5, 1));
    assert_eq!(pair_at(4), (4, 0), "data top clamps offset to 0");
    assert_eq!(pair_at(0), (0, 0));
}

#[test]
fn stateful_zero_lengths_are_safe() {
    assert_eq!(scroll_offset_for_cursor(3, 0, 8, 0), 0);
    assert_eq!(scroll_offset_for_cursor(3, 20, 0, 0), 0);
    assert_eq!(
        scroll_offset_for_cursor(0, 5, 8, 7),
        0,
        "len<=visible -> always 0"
    );
}

#[test]
fn stateful_tiny_viewport_clamps_scrolloff() {
    // viewport=3 -> effective scrolloff = 1. Walking sel=0..=9 with offset starting at 0:
    let mut off = 0usize;
    let trail: Vec<(usize, usize)> = (0..10)
        .map(|sel| {
            off = scroll_offset_for_cursor(sel, 10, 3, off);
            (sel, off)
        })
        .collect();
    // sel=0 off=0 (data top); sel=1 still 0 (margin); sel=2 -> lower threshold = 0 + 3 - 1 = 2;
    // slide: new_offset = 2 + 1 + 1 - 3 = 1.
    assert_eq!(trail[0], (0, 0));
    assert_eq!(trail[1], (1, 0));
    assert_eq!(trail[2], (2, 1));
    assert_eq!(trail[5], (5, 4));
    assert_eq!(trail[9], (9, 7), "max offset = 10 - 3 = 7");
}
