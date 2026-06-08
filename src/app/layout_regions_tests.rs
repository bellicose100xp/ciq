//! Tests for the pure layout-region hit-test (`target_at`) — the mouse coordinate mapping on the
//! pure-core hard floor. Each test pins a branch: overlay-wins, in-pane-body vs header/border,
//! query-bar text-col with the prompt offset, scroll-offset folding, and outside-everything.

use ratatui::layout::Rect;

use super::{LayoutRegions, MouseTarget, PopupKind};

const PROMPT_W: u16 = 2;

/// Regions matching the real layout: a bordered results pane filling the top, a 1-row query bar
/// below it. (Screen is 0-based; pane at y=0 height=10 -> rows 0..10; bar at y=10.)
fn base_regions() -> LayoutRegions {
    LayoutRegions {
        results_pane: Some(Rect::new(0, 0, 40, 10)),
        query_bar: Some(Rect::new(0, 10, 40, 1)),
        popup: None,
    }
}

#[test]
fn cell_outside_every_surface_is_none() {
    let r = base_regions();
    // y past the bar (row 11) is outside both the pane and the bar.
    assert_eq!(r.target_at(5, 11, PROMPT_W, 0, 0), None);
    // x past the pane width.
    assert_eq!(r.target_at(100, 3, PROMPT_W, 0, 0), None);
}

#[test]
fn click_in_pane_body_resolves_body_row_with_scroll_offset() {
    let r = base_regions();
    // Pane top border = row 0; sticky header = row 1; body begins at row 2.
    // A click on screen row 2 with no scroll is body row 0.
    assert_eq!(
        r.target_at(5, 2, PROMPT_W, 0, 0),
        Some(MouseTarget::Results { body_row: Some(0) })
    );
    // Screen row 4 with a scroll offset of 10 -> body row (4-2)+10 = 12.
    assert_eq!(
        r.target_at(5, 4, PROMPT_W, 10, 0),
        Some(MouseTarget::Results { body_row: Some(12) })
    );
}

#[test]
fn banner_row_pushes_the_body_down_by_one() {
    let r = base_regions();
    // With a 1-row truncation banner: top border = row 0, banner = row 1, sticky header = row 2,
    // body begins at row 3. The banner + header rows have no body row; row 3 is body 0.
    assert_eq!(
        r.target_at(5, 1, PROMPT_W, 0, 1),
        Some(MouseTarget::Results { body_row: None }),
        "the banner row is not a body row"
    );
    assert_eq!(
        r.target_at(5, 2, PROMPT_W, 0, 1),
        Some(MouseTarget::Results { body_row: None }),
        "the header (shifted down by the banner) is not a body row"
    );
    assert_eq!(
        r.target_at(5, 3, PROMPT_W, 0, 1),
        Some(MouseTarget::Results { body_row: Some(0) }),
        "the body begins one row lower when a banner is shown"
    );
    // The scroll offset still folds in.
    assert_eq!(
        r.target_at(5, 5, PROMPT_W, 10, 1),
        Some(MouseTarget::Results { body_row: Some(12) })
    );
}

#[test]
fn click_on_pane_border_and_header_has_no_body_row() {
    let r = base_regions();
    // Top border (row 0): inside the pane, but no body row.
    assert_eq!(
        r.target_at(5, 0, PROMPT_W, 0, 0),
        Some(MouseTarget::Results { body_row: None })
    );
    // Sticky header (row 1): inside the pane, still no body row.
    assert_eq!(
        r.target_at(5, 1, PROMPT_W, 0, 0),
        Some(MouseTarget::Results { body_row: None })
    );
    // Bottom border (row 9, the pane's last row): no body row.
    assert_eq!(
        r.target_at(5, 9, PROMPT_W, 0, 0),
        Some(MouseTarget::Results { body_row: None })
    );
}

#[test]
fn click_in_query_bar_maps_to_text_col_past_the_prompt() {
    let r = base_regions();
    // The bar is at y=10; the `> ` prompt occupies cols 0..2, so a click at col 5 is text col 3.
    assert_eq!(
        r.target_at(5, 10, PROMPT_W, 0, 0),
        Some(MouseTarget::QueryBar { row: 0, col: 3 })
    );
    // A click on the prompt itself clamps to text col 0.
    assert_eq!(
        r.target_at(0, 10, PROMPT_W, 0, 0),
        Some(MouseTarget::QueryBar { row: 0, col: 0 })
    );
    assert_eq!(
        r.target_at(1, 10, PROMPT_W, 0, 0),
        Some(MouseTarget::QueryBar { row: 0, col: 0 })
    );
}

#[test]
fn click_on_a_lower_line_of_a_multiline_bar_resolves_its_row() {
    // A multiline bar spanning 3 rows (y=8..11): a click on the 2nd visual line (y=9) resolves
    // row 1, and the 3rd line (y=10) resolves row 2 — the clicked line is not discarded.
    let r = LayoutRegions {
        results_pane: Some(Rect::new(0, 0, 40, 8)),
        query_bar: Some(Rect::new(0, 8, 40, 3)),
        popup: None,
    };
    assert_eq!(
        r.target_at(5, 8, PROMPT_W, 0, 0),
        Some(MouseTarget::QueryBar { row: 0, col: 3 })
    );
    assert_eq!(
        r.target_at(5, 9, PROMPT_W, 0, 0),
        Some(MouseTarget::QueryBar { row: 1, col: 3 })
    );
    assert_eq!(
        r.target_at(5, 10, PROMPT_W, 0, 0),
        Some(MouseTarget::QueryBar { row: 2, col: 3 })
    );
}

#[test]
fn popup_overlay_wins_over_the_pane_behind_it() {
    let mut r = base_regions();
    // A popup box over the pane (a bordered box at y=4 height=4 -> rows 4..8).
    r.popup = Some((PopupKind::Autocomplete, Rect::new(0, 4, 20, 4)));
    // A click inside the popup hits the popup, not the pane underneath.
    assert_eq!(
        r.target_at(3, 5, PROMPT_W, 0, 0),
        Some(MouseTarget::Popup {
            kind: PopupKind::Autocomplete,
            row: Some(0), // first inner row (one below the popup's top border)
        })
    );
    // A click on the popup's top border has no inner row.
    assert_eq!(
        r.target_at(3, 4, PROMPT_W, 0, 0),
        Some(MouseTarget::Popup {
            kind: PopupKind::Autocomplete,
            row: None,
        })
    );
    // A click outside the popup but inside the pane still resolves to the pane.
    assert_eq!(
        r.target_at(3, 2, PROMPT_W, 0, 0),
        Some(MouseTarget::Results { body_row: Some(0) })
    );
}

#[test]
fn popup_inner_rows_count_from_one_below_the_top_border() {
    let mut r = base_regions();
    r.popup = Some((PopupKind::History, Rect::new(0, 4, 20, 5))); // rows 4..9
    // Inner rows: row 5 -> 0, row 6 -> 1, row 7 -> 2. Bottom border (row 8) -> None.
    for (screen_row, want) in [(5u16, Some(0usize)), (6, Some(1)), (7, Some(2))] {
        assert_eq!(
            r.target_at(3, screen_row, PROMPT_W, 0, 0),
            Some(MouseTarget::Popup {
                kind: PopupKind::History,
                row: want,
            })
        );
    }
    assert_eq!(
        r.target_at(3, 8, PROMPT_W, 0, 0),
        Some(MouseTarget::Popup {
            kind: PopupKind::History,
            row: None,
        })
    );
}

#[test]
fn empty_regions_resolve_to_none() {
    let r = LayoutRegions::default();
    assert_eq!(r.target_at(0, 0, PROMPT_W, 0, 0), None);
    assert_eq!(r.target_at(10, 10, PROMPT_W, 0, 0), None);
}

#[test]
fn zero_height_pane_has_no_body_row() {
    let r = LayoutRegions {
        results_pane: Some(Rect::new(0, 0, 40, 0)),
        query_bar: None,
        popup: None,
    };
    // A zero-height pane contains no cells, so any probe is outside it.
    assert_eq!(r.target_at(5, 0, PROMPT_W, 0, 0), None);
}
