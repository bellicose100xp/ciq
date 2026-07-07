//! Tests for the deterministic double-click tracker. Time is a parameter (`now_ms`), so the
//! threshold branch is exercised with explicit timestamps — no sleeps (jiq's tracker reads
//! `Instant::now()` and needs a real 420ms sleep for this case; ciq's seam removes that).

use super::{ClickSurface, DOUBLE_CLICK_THRESHOLD_MS, DoubleClickTracker, Granularity};
use crate::app::layout_regions::PopupKind;

#[test]
fn two_fast_clicks_same_cell_pair_into_a_double() {
    let mut t = DoubleClickTracker::new();
    assert!(!t.check_and_record(100, 4, 7, ClickSurface::Results, Granularity::SameCell));
    assert!(t.check_and_record(300, 4, 7, ClickSurface::Results, Granularity::SameCell));
}

#[test]
fn a_click_at_exactly_the_threshold_still_pairs() {
    let mut t = DoubleClickTracker::new();
    t.check_and_record(0, 1, 1, ClickSurface::Results, Granularity::SameCell);
    assert!(t.check_and_record(
        DOUBLE_CLICK_THRESHOLD_MS,
        1,
        1,
        ClickSurface::Results,
        Granularity::SameCell
    ));
}

#[test]
fn a_slow_second_click_does_not_pair_and_becomes_the_new_first_half() {
    let mut t = DoubleClickTracker::new();
    t.check_and_record(0, 1, 1, ClickSurface::Results, Granularity::SameCell);
    assert!(!t.check_and_record(
        DOUBLE_CLICK_THRESHOLD_MS + 1,
        1,
        1,
        ClickSurface::Results,
        Granularity::SameCell
    ));
    // …but it seeds a fresh pair: a third fast click on the same cell IS a double.
    assert!(t.check_and_record(
        DOUBLE_CLICK_THRESHOLD_MS + 200,
        1,
        1,
        ClickSurface::Results,
        Granularity::SameCell
    ));
}

#[test]
fn same_cell_granularity_rejects_a_column_shift() {
    let mut t = DoubleClickTracker::new();
    t.check_and_record(
        0,
        4,
        7,
        ClickSurface::Popup(PopupKind::Autocomplete),
        Granularity::SameCell,
    );
    assert!(!t.check_and_record(
        100,
        5,
        7,
        ClickSurface::Popup(PopupKind::Autocomplete),
        Granularity::SameCell
    ));
}

#[test]
fn same_row_granularity_tolerates_a_column_shift() {
    let mut t = DoubleClickTracker::new();
    t.check_and_record(
        0,
        4,
        7,
        ClickSurface::Popup(PopupKind::History),
        Granularity::SameRow,
    );
    assert!(t.check_and_record(
        100,
        20,
        7,
        ClickSurface::Popup(PopupKind::History),
        Granularity::SameRow
    ));
}

#[test]
fn same_row_granularity_rejects_a_row_shift() {
    let mut t = DoubleClickTracker::new();
    t.check_and_record(
        0,
        4,
        7,
        ClickSurface::Popup(PopupKind::History),
        Granularity::SameRow,
    );
    assert!(!t.check_and_record(
        100,
        4,
        8,
        ClickSurface::Popup(PopupKind::History),
        Granularity::SameRow
    ));
}

#[test]
fn clicks_on_different_surfaces_never_pair() {
    let mut t = DoubleClickTracker::new();
    t.check_and_record(0, 4, 7, ClickSurface::Results, Granularity::SameRow);
    assert!(!t.check_and_record(
        100,
        4,
        7,
        ClickSurface::Popup(PopupKind::History),
        Granularity::SameRow
    ));
}

#[test]
fn a_completed_double_clears_state_so_a_triple_starts_fresh() {
    let mut t = DoubleClickTracker::new();
    t.check_and_record(0, 1, 1, ClickSurface::Results, Granularity::SameCell);
    assert!(t.check_and_record(100, 1, 1, ClickSurface::Results, Granularity::SameCell));
    // The third click must NOT chain into another double off the second.
    assert!(!t.check_and_record(200, 1, 1, ClickSurface::Results, Granularity::SameCell));
}

#[test]
fn reset_invalidates_a_pending_pair() {
    let mut t = DoubleClickTracker::new();
    t.check_and_record(0, 1, 1, ClickSurface::Results, Granularity::SameCell);
    t.reset();
    assert!(!t.check_and_record(100, 1, 1, ClickSurface::Results, Granularity::SameCell));
}
