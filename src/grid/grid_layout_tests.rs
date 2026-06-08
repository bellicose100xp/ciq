//! Tests for `grid::grid_layout` — alignment per type, gutters, column-granular h-scroll,
//! null glyph, plus the two property tests (1 row == 1 body line; widths fit the viewport).

use proptest::prelude::*;

use crate::engine::{Cell, Column, Table};
use crate::grid::grid_layout::{Align, GridView, layout_grid};
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
        let view = GridView { width: w, height: h, h_col_offset: hoff, v_row_offset: voff };
        let _ = layout_grid(&t, &view);
    }
}
