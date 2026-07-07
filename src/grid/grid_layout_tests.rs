//! Tests for `grid::grid_layout` — alignment per type, gutters, column-granular h-scroll,
//! null glyph, plus the two property tests (1 row == 1 body line; widths fit the viewport).

use proptest::prelude::*;

use crate::engine::{Cell, Column, Table};
use crate::grid::grid_layout::{
    Align, GridView, columns_dropped_at, h_col_offset_to_reveal, layout_grid, prefix_left_edge,
};
use crate::schema::ColumnType;

fn sample_table() -> Table {
    Table::new(vec![
        Column::new(
            "id",
            ColumnType::Int,
            vec![Cell::Int(1), Cell::Int(20), Cell::Int(300)],
        ),
        Column::new(
            "name",
            ColumnType::Text,
            vec![
                Cell::Text("Ada".into()),
                Cell::Text("Bo".into()),
                Cell::Null,
            ],
        ),
    ])
}

#[test]
fn align_for_type_matches_column_type_helper() {
    assert_eq!(Align::for_type(&ColumnType::Int), Align::Right);
    assert_eq!(Align::for_type(&ColumnType::Float), Align::Right);
    assert_eq!(Align::for_type(&ColumnType::Date), Align::Right);
    assert_eq!(Align::for_type(&ColumnType::Timestamp), Align::Right);
    assert_eq!(Align::for_type(&ColumnType::Text), Align::Left);
    assert_eq!(Align::for_type(&ColumnType::Bool), Align::Left);
    assert_eq!(Align::for_type(&ColumnType::Other("X".into())), Align::Left);
}

#[test]
fn one_body_line_per_row() {
    let t = sample_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    assert_eq!(frame.body.len(), t.row_count());
}

#[test]
fn header_carries_column_names_with_type_badges() {
    let t = sample_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    // The single sticky header now carries `name (badge)` per column (the folded-in type label).
    assert!(frame.header.contains("id (int)"));
    assert!(frame.header.contains("name (txt)"));
}

#[test]
fn numeric_column_right_aligned_text_left_aligned() {
    let t = sample_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    // Widths are sized to the `name (badge)` header label: id = max("id (int)"=8, cells<=3) = 8;
    // name = max("name (txt)"=10, "Ada"=3,"NULL"=4) = 10.
    assert_eq!(frame.widths, vec![8, 10]);
    assert_eq!(frame.aligns, vec![Align::Right, Align::Left]);
    // Row 0: id=1 right-aligned in width 8 -> "       1"; name="Ada" left in 10 -> "Ada       ".
    assert_eq!(frame.body[0].text, "       1  Ada       ");
    // Row 1: id=20 -> "      20"; name="Bo" -> "Bo        ".
    assert_eq!(frame.body[1].text, "      20  Bo        ");
}

#[test]
fn null_cell_renders_glyph_in_body() {
    let t = sample_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    // Row 2: id=300 right-aligned in width 8 -> "     300"; name=NULL left in 10 -> "NULL      ".
    assert_eq!(frame.body[2].text, "     300  NULL      ");
}

#[test]
fn null_span_marks_only_the_genuine_null_cell() {
    // A row mixing a present value, a literal text "NULL", and a real SQL NULL: only the byte
    // range of the genuine `Cell::Null` is flagged, so the renderer dims it alone.
    let t = Table::new(vec![
        Column::new("a", ColumnType::Text, vec![Cell::Text("x".into())]),
        Column::new("b", ColumnType::Text, vec![Cell::Text("NULL".into())]),
        Column::new("c", ColumnType::Text, vec![Cell::Null]),
    ]);
    let frame = layout_grid(&t, &GridView::new(80, 24));
    let row = &frame.body[0];
    // Exactly one null span, and it covers the third cell (the genuine NULL), not the literal
    // text "NULL" in column b.
    assert_eq!(row.null_spans.len(), 1, "only the real NULL is flagged");
    let span = row.null_spans[0].clone();
    // The flagged range is column c's padded cell, which contains the glyph "NULL".
    assert!(row.text[span.clone()].contains("NULL"));
    // The flagged range is the LAST "NULL" in the row (column c), not column b's text "NULL".
    assert_eq!(span.start, row.text.rfind("NULL").unwrap());
}

#[test]
fn substring_null_inside_word_is_not_flagged() {
    // "ANNULLED" contains the substring "NULL" but is a present value: no null span.
    let t = Table::new(vec![Column::new(
        "w",
        ColumnType::Text,
        vec![Cell::Text("ANNULLED".into())],
    )]);
    let frame = layout_grid(&t, &GridView::new(80, 24));
    assert!(
        frame.body[0].null_spans.is_empty(),
        "a present value containing 'NULL' must not be flagged null"
    );
}

#[test]
fn truncated_null_glyph_still_flagged() {
    // A NULL in a column narrower than the 4-char glyph truncates to "N…"/"NU…" — the literal
    // substring "NULL" is gone, but the cell is still flagged so the renderer dims it.
    let t = Table::new(vec![Column::new("c", ColumnType::Text, vec![Cell::Null])]);
    // Cap the column below 4 chars via a 2-wide viewport budget.
    let frame = layout_grid(&t, &GridView::new(2, 24));
    let row = &frame.body[0];
    assert_eq!(row.null_spans.len(), 1, "truncated NULL is still flagged");
    assert!(
        !row.text[row.null_spans[0].clone()].contains("NULL"),
        "the flagged text is truncated, not the literal glyph"
    );
}

#[test]
fn col_x_tracks_gutters() {
    let t = sample_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    // first col (`id (int)` width 8) starts at 0; second at width(8) + gap(2) = 10.
    assert_eq!(frame.col_x, vec![0, 10]);
    // total = col1(8) + gap(2) + col2(10) = 20.
    assert_eq!(frame.total_width, 20);
}

#[test]
fn h_col_offset_drops_leading_columns() {
    let t = sample_table();
    let mut view = GridView::new(80, 24);
    view.h_col_offset = 1;
    let frame = layout_grid(&t, &view);
    // Only the "name" column is visible now.
    assert_eq!(frame.widths.len(), 1);
    assert_eq!(frame.aligns, vec![Align::Left]);
    assert!(frame.header.contains("name"));
    assert!(!frame.header.contains("id"));
    assert_eq!(frame.col_x, vec![0]);
}

#[test]
fn h_col_offset_past_end_yields_empty_visible() {
    let t = sample_table();
    let mut view = GridView::new(80, 24);
    view.h_col_offset = 99;
    let frame = layout_grid(&t, &view);
    assert!(frame.widths.is_empty());
    assert!(frame.col_x.is_empty());
    assert_eq!(frame.header, "");
    assert_eq!(frame.total_width, 0);
    // Body lines still 1 per row, but each empty (no visible columns).
    assert_eq!(frame.body.len(), t.row_count());
    assert!(frame.body.iter().all(|l| l.is_empty()));
}

// --- char-grain horizontal slide (trackpad axis) ---

#[test]
fn h_char_offset_zero_yields_zero_body_scroll() {
    let t = sample_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    // No char slide; the body scroll is 0.
    assert_eq!(frame.body_scroll_chars, 0);
}

#[test]
fn h_char_offset_within_first_column_slides_paragraph() {
    let t = sample_table();
    let mut view = GridView::new(80, 24);
    view.h_char_offset = 4;
    // Still inside col 0 (first col is at least 4 chars wide); h_col_offset stays 0 by design.
    let frame = layout_grid(&t, &view);
    assert_eq!(
        frame.body_scroll_chars, 4,
        "char slide rides INSIDE the leftmost visible column"
    );
}

#[test]
fn h_char_offset_with_h_col_offset_subtracts_dropped_left_edge() {
    let t = sample_table();
    let mut view = GridView::new(80, 24);
    // Drop col 0 (h_col_offset = 1). Col 0's left-edge-after = width(8 chars) + gutter(2) = 10.
    // A user with `h_char_offset = 12` has slid the trackpad 2 chars INTO col 1.
    view.h_col_offset = 1;
    view.h_char_offset = 12;
    let frame = layout_grid(&t, &view);
    assert_eq!(
        frame.body_scroll_chars, 2,
        "12 chars slid - 10 chars dropped = 2 chars into col 1"
    );
}

#[test]
fn prefix_left_edge_includes_trailing_gutter() {
    // Empty -> 0; one column -> w + gutter; two columns -> w0 + gutter + w1 + gutter.
    assert_eq!(prefix_left_edge(&[]), 0);
    assert_eq!(prefix_left_edge(&[3]), 5);
    assert_eq!(prefix_left_edge(&[3, 7]), 14);
}

#[test]
fn columns_dropped_at_returns_largest_fully_off_column_count() {
    let widths = [3u16, 7, 5];
    // 0 chars: nothing dropped.
    assert_eq!(columns_dropped_at(&widths, 0), 0);
    // 4 chars: still inside col 0 (col 1 starts at 5); 0 dropped.
    assert_eq!(columns_dropped_at(&widths, 4), 0);
    // 5 chars: col 1 starts here; col 0 fully off-screen.
    assert_eq!(columns_dropped_at(&widths, 5), 1);
    // 14 chars: cols 0 + 1 fully off.
    assert_eq!(columns_dropped_at(&widths, 14), 2);
    // Past the end: all dropped.
    assert_eq!(columns_dropped_at(&widths, 999), 3);
}

#[test]
fn h_col_offset_to_reveal_scrolls_left_with_margin() {
    let widths = [10u16, 10, 10, 10, 10];
    // Currently showing from col 2; reveal col 2 with margin 1 -> slide so col 1 leads.
    assert_eq!(h_col_offset_to_reveal(&widths, 30, 2, 2, 1), 1);
    // Reveal col 0: clamps to 0 (data start, can sit flush).
    assert_eq!(h_col_offset_to_reveal(&widths, 30, 0, 3, 1), 0);
    // Reveal col 1 with margin 1 -> col 0 leads.
    assert_eq!(h_col_offset_to_reveal(&widths, 30, 1, 3, 1), 0);
}

#[test]
fn h_col_offset_to_reveal_scrolls_right_with_margin() {
    // Five 10-wide columns, viewport fits 3 (10+2+10+2+10 = 34 > 30, so 2 full + gap). Reveal a
    // column off the right edge and confirm the offset advances so it (plus a right margin) shows.
    let widths = [10u16, 10, 10, 10, 10];
    // Start at offset 0; reveal col 4 (last) with margin 1: margin clamps to the last column, so
    // col 4 must be visible as the last column. With viewport 30, ~2 columns fit -> offset 3.
    let off = h_col_offset_to_reveal(&widths, 30, 4, 0, 1);
    assert!(off >= 3, "scrolled right to reveal col 4, got {off}");
    assert!(off <= 4, "never past the last column, got {off}");
}

#[test]
fn h_col_offset_to_reveal_leaves_offset_when_target_already_inside() {
    let widths = [10u16, 10, 10, 10, 10];
    // Offset 1 shows cols 1,2(,3 partial); target col 2 with margin 0 is already inside -> no move.
    assert_eq!(h_col_offset_to_reveal(&widths, 30, 2, 1, 0), 1);
}

#[test]
fn h_col_offset_to_reveal_clamps_and_handles_empty() {
    assert_eq!(h_col_offset_to_reveal(&[], 30, 0, 0, 1), 0);
    let widths = [5u16, 5, 5];
    // Target past the end clamps to the last column.
    let off = h_col_offset_to_reveal(&widths, 20, 99, 0, 1);
    assert!(off <= 2);
}

#[test]
fn narrow_viewport_shows_at_least_one_column() {
    let t = sample_table();
    // viewport width 1 is far below the first column's width, but one column always shows.
    let frame = layout_grid(&t, &GridView::new(1, 24));
    assert_eq!(frame.col_x.len(), 1);
}

#[test]
fn viewport_truncates_trailing_columns_that_dont_fit() {
    let t = Table::new(vec![
        Column::new("a", ColumnType::Text, vec![Cell::Text("aaaa".into())]),
        Column::new("b", ColumnType::Text, vec![Cell::Text("bbbb".into())]),
        Column::new("c", ColumnType::Text, vec![Cell::Text("cccc".into())]),
    ]);
    // Each col is sized to its `x (txt)`=7 header label, gutter 2. Two cols = 7+2+7 = 16;
    // three = 7+2+7+2+7 = 25. Viewport 18 fits two.
    let frame = layout_grid(&t, &GridView::new(18, 24));
    assert_eq!(frame.widths.len(), 2);
}

#[test]
fn empty_table_yields_empty_header_and_body() {
    let t = Table::default();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    assert_eq!(frame.header, "");
    assert!(frame.body.is_empty());
    assert_eq!(frame.total_width, 0);
}

#[test]
fn single_row_yields_single_body_line() {
    let t = Table::new(vec![Column::new("x", ColumnType::Int, vec![Cell::Int(7)])]);
    let frame = layout_grid(&t, &GridView::new(80, 24));
    assert_eq!(frame.body.len(), 1);
}

// ---- property tests ----

fn arb_table() -> impl Strategy<Value = Table> {
    // 1..=4 columns, each 1..=6 rows (uniform across columns), simple ascii text cells.
    (1usize..=4, 1usize..=6).prop_flat_map(|(ncols, nrows)| {
        let col = (
            "[a-z]{1,8}",
            prop::collection::vec("[a-zA-Z0-9 ]{0,12}", nrows),
        )
            .prop_map(move |(name, vals)| {
                Column::new(
                    name,
                    ColumnType::Text,
                    vals.into_iter().map(Cell::Text).collect(),
                )
            });
        prop::collection::vec(col, ncols).prop_map(Table::new)
    })
}

proptest! {
    #[test]
    fn prop_one_body_line_per_row(t in arb_table(), w in 1u16..120, h in 1u16..40) {
        let frame = layout_grid(&t, &GridView::new(w, h));
        prop_assert_eq!(frame.body.len(), t.row_count());
    }

    #[test]
    fn prop_total_width_fits_viewport(t in arb_table(), w in 1u16..120) {
        let frame = layout_grid(&t, &GridView::new(w, 24));
        // Either everything fits the viewport, or exactly one (too-wide) column is shown.
        if frame.widths.len() > 1 {
            prop_assert!(
                frame.total_width <= w,
                "total {} > viewport {} with {} cols",
                frame.total_width, w, frame.widths.len()
            );
        }
    }

    #[test]
    fn prop_col_x_sum_consistent(t in arb_table(), w in 1u16..120) {
        let frame = layout_grid(&t, &GridView::new(w, 24));
        // col_x, widths, aligns are parallel.
        prop_assert_eq!(frame.col_x.len(), frame.widths.len());
        prop_assert_eq!(frame.col_x.len(), frame.aligns.len());
        // Each col_x + width <= total_width.
        for (x, wd) in frame.col_x.iter().zip(&frame.widths) {
            prop_assert!(x + wd <= frame.total_width);
        }
    }

    #[test]
    fn prop_never_panics_on_arbitrary_view(
        t in arb_table(),
        w in 0u16..200,
        h in 0u16..200,
        hoff in 0usize..10,
        voff in 0usize..10,
    ) {
        let view = GridView {
            width: w,
            height: h,
            h_col_offset: hoff,
            h_char_offset: 0,
            v_row_offset: voff,
        };
        let _ = layout_grid(&t, &view);
    }
}
