//! Tests for `schema_bar` — the always-visible column/type bar above the grid (`dev/PLAN.md`
//! §6.3). The pure layout (`layout_schema_bar`, `summary`) is asserted directly: span text, which
//! span is active, dead-on alignment to the grid's `col_x`, truncation, and per-type badge text.
//! The blit (`render_schema_bar`) is `TestBackend`-snapshot-tested (headless; NOT shell-exempt).

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::Span;

use super::{fit_entry, layout_schema_bar, render_schema_bar, summary};
use crate::engine::{Cell, Column, Table};
use crate::grid::grid_layout::{GridFrame, GridView, layout_grid};
use crate::schema::{ColumnMeta, ColumnType, Schema};
use crate::theme;

/// A schema matching `typed_table` below (same names + types, same order).
fn typed_schema() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("name", ColumnType::Text),
        ColumnMeta::new("amount", ColumnType::Float),
    ])
}

/// Cells are wide enough that every column's grid width admits the decorated `name (badge)` form
/// (so the bar shows the badges, not the bare-name fallback). The widths land at: `id` 10
/// (`1000000000`), `name` 11 (`Augustine`-class), `amount` 12 (`amount (num)`-class) — all >= their
/// decorated label width.
fn typed_table() -> Table {
    Table::new(vec![
        Column::new(
            "id",
            ColumnType::Int,
            vec![Cell::Int(1), Cell::Int(2), Cell::Int(1_000_000_000)],
        ),
        Column::new(
            "name",
            ColumnType::Text,
            vec![
                Cell::Text("Ada Lovelace".into()),
                Cell::Text("Bo".into()),
                Cell::Text("Cy".into()),
            ],
        ),
        Column::new(
            "amount",
            ColumnType::Float,
            vec![
                Cell::Float(12.5),
                Cell::Float(7.0),
                Cell::Float(1234567890.5),
            ],
        ),
    ])
}

/// The plain text of a span list, concatenated (the on-screen line content).
fn joined(spans: &[Span<'static>]) -> String {
    spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Whether a span carries the underline modifier the active style uses.
fn is_active(s: &Span<'static>) -> bool {
    s.style.add_modifier.contains(Modifier::UNDERLINED)
}

#[test]
fn empty_schema_or_empty_grid_yields_no_spans() {
    let empty_schema = Schema::default();
    let grid = layout_grid(&typed_table(), &GridView::new(80, 24));
    assert!(layout_schema_bar(&empty_schema, &grid, 0, None).is_empty());

    // A grid that shows no columns (degenerate empty frame).
    let blank = GridFrame {
        header: String::new(),
        body: Vec::new(),
        col_x: Vec::new(),
        widths: Vec::new(),
        aligns: Vec::new(),
        total_width: 0,
    };
    assert!(layout_schema_bar(&typed_schema(), &blank, 0, None).is_empty());
}

#[test]
fn spans_carry_name_and_badge_per_column() {
    let schema = typed_schema();
    let grid = layout_grid(&typed_table(), &GridView::new(80, 24));
    let spans = layout_schema_bar(&schema, &grid, 0, None);
    let line = joined(&spans);
    // Each column shows `name (badge)` with the type's badge text.
    assert!(line.contains("id (int)"));
    assert!(line.contains("name (txt)"));
    assert!(line.contains("amount (num)"));
}

#[test]
fn badge_text_per_type() {
    // The bar uses ColumnType::badge directly — one assertion per kind so a badge regression here
    // is caught even if the central badge map changes.
    let cases = [
        (ColumnType::Int, "int"),
        (ColumnType::Float, "num"),
        (ColumnType::Bool, "bool"),
        (ColumnType::Date, "date"),
        (ColumnType::Timestamp, "ts"),
        (ColumnType::Text, "txt"),
        (ColumnType::Other("HUGEINT".into()), "oth"),
    ];
    for (ty, badge) in cases {
        let schema = Schema::new(vec![ColumnMeta::new("col", ty.clone())]);
        // A cell wide enough that `col (badge)` (<= 10 chars) fits, so the badge is shown.
        let table = Table::new(vec![Column::new(
            "col",
            ty,
            vec![Cell::Text("wide enough value".into())],
        )]);
        let grid = layout_grid(&table, &GridView::new(80, 24));
        let line = joined(&layout_schema_bar(&schema, &grid, 0, None));
        assert!(
            line.contains(&format!("col ({badge})")),
            "expected badge `{badge}` in `{line}`"
        );
    }
}

#[test]
fn active_column_span_is_styled_distinctly() {
    let schema = typed_schema();
    let grid = layout_grid(&typed_table(), &GridView::new(80, 24));

    // Mark column 1 (`name`) active.
    let spans = layout_schema_bar(&schema, &grid, 0, Some(1));
    let active: Vec<&Span<'static>> = spans.iter().filter(|s| is_active(s)).collect();
    assert_eq!(active.len(), 1, "exactly one span is active");
    assert!(
        active[0].content.contains("name"),
        "the active span is the `name` column"
    );
    assert_eq!(active[0].style, theme::schema_bar::active());

    // No active column -> no active span.
    let none = layout_schema_bar(&schema, &grid, 0, None);
    assert!(!none.iter().any(is_active));
}

#[test]
fn active_column_scrolled_off_renders_no_active_span() {
    let schema = typed_schema();
    // Active column 0 (`id`), but we scrolled it off the left edge: h_col_offset starts the
    // visible window at column 1, so the absolute active index 0 is not in view.
    let spans = layout_schema_bar(&schema, &grid_after_scroll(&typed_table(), 1), 1, Some(0));
    assert!(
        !spans.iter().any(is_active),
        "an active column scrolled off shows no active span"
    );
    // But the entries shown start at the scrolled-to column (`name`).
    let line = joined(&spans);
    assert!(line.contains("name (txt)"));
    assert!(!line.contains("id (int)"));
}

/// Lay out the grid with `h_col_offset` leading columns scrolled off.
fn grid_after_scroll(table: &Table, h_col_offset: usize) -> GridFrame {
    let mut view = GridView::new(80, 24);
    view.h_col_offset = h_col_offset;
    layout_grid(table, &view)
}

#[test]
fn each_column_span_starts_at_its_grid_col_x() {
    // The dead-on alignment guarantee (§6.3): the start char offset of each column's label span
    // equals the grid's `col_x` for that column. We walk the span list accumulating char widths;
    // entry spans land exactly on col_x, gap spans bridge the gutter.
    let schema = typed_schema();
    let grid = layout_grid(&typed_table(), &GridView::new(80, 24));
    let spans = layout_schema_bar(&schema, &grid, 0, None);

    // The label spans are at even indices (0, 2, 4, …); odd indices are the gutter gaps.
    let mut char_pos = 0usize;
    let mut visible_col = 0usize;
    for (i, span) in spans.iter().enumerate() {
        if i % 2 == 0 {
            // A column-label span — must start at the grid's col_x for this visible column.
            assert_eq!(
                char_pos as u16, grid.col_x[visible_col],
                "column {visible_col} label starts at col_x"
            );
            visible_col += 1;
        }
        char_pos += span.content.chars().count();
    }
    assert_eq!(
        visible_col,
        grid.col_x.len(),
        "all visible columns accounted for"
    );
}

#[test]
fn each_label_span_width_matches_grid_column_width() {
    // Every entry span is padded to exactly the grid column's width, which is what makes the
    // start-at-col_x invariant above hold for the *next* column too.
    let schema = typed_schema();
    let grid = layout_grid(&typed_table(), &GridView::new(80, 24));
    let spans = layout_schema_bar(&schema, &grid, 0, None);
    let label_spans: Vec<&Span<'static>> = spans.iter().step_by(2).collect();
    assert_eq!(label_spans.len(), grid.widths.len());
    for (span, &w) in label_spans.iter().zip(&grid.widths) {
        assert_eq!(span.content.chars().count() as u16, w);
    }
}

#[test]
fn entry_truncates_with_ellipsis_when_too_narrow() {
    // `fit_entry` first tries `name (badge)`, then falls back to the truncated name when the
    // decorated form overflows.
    // Decorated form fits exactly.
    assert_eq!(fit_entry("id", "int", 8), "id (int)");
    // Padded to width when short.
    assert_eq!(fit_entry("id", "int", 10), "id (int)  ");
    // Too narrow for `name (txt)` (10 chars) -> falls back to the name, here it fits in 6.
    assert_eq!(fit_entry("name", "txt", 6), "name  ");
    // Narrower than even the name -> ellipsis-truncated name.
    assert_eq!(fit_entry("amount", "num", 4), "amo…");
    // Width 0 -> empty.
    assert_eq!(fit_entry("id", "int", 0), "");
}

#[test]
fn fit_entry_never_panics_on_multibyte_name() {
    // A multi-byte header truncated below its char width must not slice a char boundary.
    let _ = fit_entry("¡cañón!", "txt", 3);
    let _ = fit_entry("日本語カラム", "txt", 4);
    // (Reaching here without a panic is the assertion.)
}

#[test]
fn summary_string() {
    assert_eq!(summary(Some(','), true), "delim , | header on");
    assert_eq!(summary(Some(';'), false), "delim ; | header off");
    assert_eq!(summary(Some('|'), true), "delim | | header on");
    // A tab is shown as a visible escape so it doesn't render as whitespace.
    assert_eq!(summary(Some('\t'), true), "delim \\t | header on");
    // Auto-detected delimiter.
    assert_eq!(summary(None, true), "delim auto | header on");
}

#[test]
fn layout_span_list_snapshot() {
    let schema = typed_schema();
    let grid = layout_grid(&typed_table(), &GridView::new(80, 24));
    let spans = layout_schema_bar(&schema, &grid, 0, Some(1));
    // Snapshot the (text, active?) shape so a layout/badge/active regression is caught.
    let shape: Vec<(String, bool)> = spans
        .iter()
        .map(|s| (s.content.to_string(), is_active(s)))
        .collect();
    insta::assert_debug_snapshot!(shape);
}

#[test]
fn render_schema_bar_snapshot_80x1() {
    let schema = typed_schema();
    let grid = layout_grid(&typed_table(), &GridView::new(80, 24));
    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).expect("TestBackend terminal");
    terminal
        .draw(|f| {
            render_schema_bar(f, Rect::new(0, 0, 80, 1), &schema, &grid, 0, Some(1));
        })
        .expect("draw to TestBackend");
    insta::assert_snapshot!(terminal.backend().to_string());
}

#[test]
fn render_schema_bar_no_op_on_zero_area() {
    let schema = typed_schema();
    let grid = layout_grid(&typed_table(), &GridView::new(80, 24));
    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            render_schema_bar(f, Rect::new(0, 0, 0, 0), &schema, &grid, 0, None);
        })
        .unwrap();
    // No panic; nothing rendered.
}
