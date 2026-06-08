//! Tests for `grid::grid_render` — the blit shim, snapshot-tested via `ratatui::TestBackend`
//! (headless; NOT shell-exempt). Includes a pathological-width case forcing ellipsis/h-scroll.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::style_body_line;
use crate::engine::{Cell, Column, Table};
use crate::grid::grid_layout::{BodyRow, GridView, layout_grid};
use crate::grid::grid_render::{body_viewport_height, render_grid};
use crate::theme;

fn render_to_string(table: &Table, view: GridView, w: u16, h: u16, v_row_offset: usize) -> String {
    let frame = layout_grid(table, &view);
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).expect("TestBackend terminal");
    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, w, h);
            render_grid(f, area, &frame, v_row_offset, false);
        })
        .expect("draw to TestBackend");
    terminal.backend().to_string()
}

fn typed_table() -> Table {
    Table::new(vec![
        Column::new(
            "id",
            crate::schema::ColumnType::Int,
            vec![Cell::Int(1), Cell::Int(2), Cell::Int(300)],
        ),
        Column::new(
            "name",
            crate::schema::ColumnType::Text,
            vec![
                Cell::Text("Ada".into()),
                Cell::Text("Bo".into()),
                Cell::Null,
            ],
        ),
        Column::new(
            "amount",
            crate::schema::ColumnType::Float,
            vec![Cell::Float(12.5), Cell::Float(7.0), Cell::Float(0.25)],
        ),
    ])
}

#[test]
fn body_viewport_height_reserves_one_row_for_header() {
    assert_eq!(body_viewport_height(24), 23);
    assert_eq!(body_viewport_height(1), 0);
    assert_eq!(body_viewport_height(0), 0);
}

#[test]
fn render_does_not_panic_on_zero_height() {
    let t = typed_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| render_grid(f, Rect::new(0, 0, 80, 0), &frame, 0, false))
        .unwrap();
    // No panic; nothing asserted beyond that.
}

#[test]
fn header_stays_put_when_body_scrolls() {
    let t = typed_table();
    let view = GridView::new(80, 6);
    let unscrolled = render_to_string(&t, view, 80, 6, 0);
    let scrolled = render_to_string(&t, view, 80, 6, 1);
    // The first rendered row (the header) is identical regardless of body scroll.
    let head0 = unscrolled.lines().next().unwrap();
    let head1 = scrolled.lines().next().unwrap();
    assert_eq!(head0, head1);
    // But the body differs (it scrolled).
    assert_ne!(unscrolled, scrolled);
}

#[test]
fn snapshot_basic_grid_80x24() {
    let t = typed_table();
    let screen = render_to_string(&t, GridView::new(80, 24), 80, 24, 0);
    insta::assert_snapshot!(screen);
}

#[test]
fn snapshot_pathological_wide_column_forces_ellipsis() {
    // One very wide column far past DEFAULT_MAX_COL_WIDTH (40), plus a narrow neighbor: the
    // wide cell must ellipsis-truncate, and the narrow viewport forces h-scroll behavior.
    let wide = "x".repeat(120);
    let t = Table::new(vec![
        Column::new(
            "huge",
            crate::schema::ColumnType::Text,
            vec![Cell::Text(wide), Cell::Text("short".into())],
        ),
        Column::new(
            "n",
            crate::schema::ColumnType::Int,
            vec![Cell::Int(1), Cell::Int(2)],
        ),
    ]);
    let screen = render_to_string(&t, GridView::new(80, 8), 80, 8, 0);
    insta::assert_snapshot!(screen);
}

#[test]
fn snapshot_null_glyph_distinct_from_empty() {
    let t = Table::new(vec![Column::new(
        "val",
        crate::schema::ColumnType::Text,
        vec![
            Cell::Null,
            Cell::Text(String::new()),
            Cell::Text("x".into()),
        ],
    )]);
    let screen = render_to_string(&t, GridView::new(20, 6), 20, 6, 0);
    insta::assert_snapshot!(screen);
}

/// True iff `style` carries the dim modifier the null style uses (and nothing else does).
fn is_null_styled(style: ratatui::style::Style) -> bool {
    style.add_modifier.contains(Modifier::DIM)
}

#[test]
fn only_genuine_null_span_is_dimmed_not_literal_text_null() {
    // Layout flags only the real `Cell::Null`; a present `Cell::Text("NULL")` and an "ANNULLED"
    // value share the same glyph text but must NOT be dimmed (the Q12 null-vs-text distinction).
    let t = Table::new(vec![
        Column::new("a", crate::schema::ColumnType::Text, vec![Cell::Null]),
        Column::new(
            "b",
            crate::schema::ColumnType::Text,
            vec![Cell::Text("NULL".into())],
        ),
        Column::new(
            "c",
            crate::schema::ColumnType::Text,
            vec![Cell::Text("ANNULLED".into())],
        ),
    ]);
    let frame = layout_grid(&t, &GridView::new(80, 24));
    let line = style_body_line(&frame.body[0], Modifier::empty());

    // Collect (text, dimmed) per span, then check exactly the first cell's "NULL" is dimmed.
    let dimmed: String = line
        .spans
        .iter()
        .filter(|s| is_null_styled(s.style))
        .map(|s| s.content.as_ref())
        .collect();
    let present: String = line
        .spans
        .iter()
        .filter(|s| !is_null_styled(s.style))
        .map(|s| s.content.as_ref())
        .collect();
    // Only one "NULL" run is dimmed (the genuine null cell).
    assert_eq!(dimmed.matches("NULL").count(), 1);
    // The literal text "NULL" (column b) and "ANNULLED" (column c) are in the un-dimmed runs.
    assert!(present.contains("ANNULLED"));
    assert!(
        present.contains("NULL"),
        "column b's literal NULL stays normal"
    );
    // Sanity: the dim span uses the theme null style.
    let null_span = line
        .spans
        .iter()
        .find(|s| is_null_styled(s.style))
        .expect("a dimmed span");
    assert_eq!(null_span.style, theme::grid::null());
}

#[test]
fn truncated_null_in_narrow_column_is_still_dimmed() {
    // A NULL in a column capped below the 4-char glyph truncates to "N…" — the literal substring
    // "NULL" is gone, but the cell is still dimmed because null-ness is carried from layout.
    let t = Table::new(vec![Column::new(
        "c",
        crate::schema::ColumnType::Text,
        vec![Cell::Null],
    )]);
    let frame = layout_grid(&t, &GridView::new(2, 24));
    let line = style_body_line(&frame.body[0], Modifier::empty());
    let dimmed_text: String = line
        .spans
        .iter()
        .filter(|s| is_null_styled(s.style))
        .map(|s| s.content.as_ref())
        .collect();
    assert!(
        !dimmed_text.is_empty(),
        "the truncated NULL glyph is dimmed"
    );
    assert!(
        !dimmed_text.contains("NULL"),
        "the dimmed text is the truncated glyph, not the literal NULL"
    );
}

#[test]
fn no_null_row_is_a_single_normal_span() {
    let row = BodyRow {
        text: "  1  Ada ".to_string(),
        null_spans: Vec::new(),
    };
    let line = style_body_line(&row, Modifier::empty());
    assert_eq!(line.spans.len(), 1);
    assert_eq!(line.spans[0].style, theme::grid::cell());
    assert!(!is_null_styled(line.spans[0].style));
}
