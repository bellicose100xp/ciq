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
fn header_carries_column_names() {
    let t = sample_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    assert!(frame.header.contains("id"));
    assert!(frame.header.contains("name"));
}

#[test]
fn numeric_column_right_aligned_text_left_aligned() {
    let t = sample_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    // id width = max("id"=2, "300"=3) = 3; name width = max("name"=4,"Ada"=3,"NULL"=4)=4.
    assert_eq!(frame.widths, vec![3, 4]);
    assert_eq!(frame.aligns, vec![Align::Right, Align::Left]);
    // Row 0: id=1 right-aligned in width 3 -> "  1"; name="Ada" left in 4 -> "Ada ".
    assert_eq!(frame.body[0], "  1  Ada ");
    // Row 1: id=20 -> " 20"; name="Bo" -> "Bo  ".
    assert_eq!(frame.body[1], " 20  Bo  ");
}

#[test]
fn null_cell_renders_glyph_in_body() {
    let t = sample_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    // Row 2: id=300 -> "300"; name=NULL -> "NULL".
    assert_eq!(frame.body[2], "300  NULL");
}

#[test]
fn col_x_tracks_gutters() {
    let t = sample_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    // first col starts at 0; second at width(3) + gap(2) = 5.
    assert_eq!(frame.col_x, vec![0, 5]);
    // total = col1(3) + gap(2) + col2(4) = 9.
    assert_eq!(frame.total_width, 9);
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
    // each col width 4, gutter 2. Two cols = 4+2+4 = 10. Three = 16. Viewport 12 fits two.
    let frame = layout_grid(&t, &GridView::new(12, 24));
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
