//! Tests for `grid::grid_render` — the blit shim, snapshot-tested via `ratatui::TestBackend`
//! (headless; NOT shell-exempt). Includes a pathological-width case forcing ellipsis/h-scroll.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::style_body_line;
use crate::engine::{Cell, Column, Table};
use crate::grid::grid_layout::{BodyRow, GridView, layout_grid};
use crate::grid::grid_render::{GridPaint, body_viewport_height, render_grid};
use crate::theme;

fn render_to_string(table: &Table, view: GridView, w: u16, h: u16, v_row_offset: usize) -> String {
    let frame = layout_grid(table, &view);
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).expect("TestBackend terminal");
    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, w, h);
            render_grid(
                f,
                area,
                &frame,
                GridPaint {
                    v_row_offset,
                    accent: theme::base::CYAN,
                    current_match_row: None,
                    ..Default::default()
                },
            );
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
        .draw(|f| {
            render_grid(
                f,
                Rect::new(0, 0, 80, 0),
                &frame,
                GridPaint {
                    accent: theme::base::CYAN,
                    current_match_row: None,
                    ..Default::default()
                },
            )
        })
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
    let line = style_body_line(&frame.body[0], Modifier::empty(), "", false);

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
    let line = style_body_line(&frame.body[0], Modifier::empty(), "", false);
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
    let text = "  1  Ada ".to_string();
    let whole = 0..text.len();
    let row = BodyRow {
        text,
        null_spans: Vec::new(),
        cell_spans: vec![whole],
    };
    let line = style_body_line(&row, Modifier::empty(), "", false);
    assert_eq!(line.spans.len(), 1);
    assert_eq!(line.spans[0].style, theme::grid::cell());
    assert!(!is_null_styled(line.spans[0].style));
}

// --- Ctrl+F match highlighting ---

/// True iff `style` is the search-match highlight.
fn is_match_styled(style: ratatui::style::Style) -> bool {
    style.bg == theme::grid::search_match().bg && style.add_modifier.contains(Modifier::BOLD)
}

#[test]
fn search_needle_highlights_matching_runs_case_insensitively() {
    let t = typed_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    // Row 0 is `1  Ada  12.5`; needle "ada" must highlight the "Ada" run only.
    let line = style_body_line(&frame.body[0], Modifier::empty(), "ada", false);
    let matched: String = line
        .spans
        .iter()
        .filter(|s| is_match_styled(s.style))
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(matched, "Ada");
    let full: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(full, frame.body[0].text, "highlighting never alters text");
}

/// True iff `style` is the CURRENT-match highlight (distinct from the dim other-match style).
fn is_current_match_styled(style: ratatui::style::Style) -> bool {
    style.bg == theme::grid::current_match().bg
}

#[test]
fn current_match_row_uses_the_distinct_current_style() {
    let t = typed_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    // is_current = true: the "Ada" run takes the current-match bg, NOT the plain search-match bg.
    let line = style_body_line(&frame.body[0], Modifier::empty(), "ada", true);
    let cur: String = line
        .spans
        .iter()
        .filter(|s| is_current_match_styled(s.style))
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(cur, "Ada", "the current match uses the current-match style");
    assert!(
        line.spans.iter().all(|s| !is_match_styled(s.style)),
        "no run uses the non-current (dim) match style on the current row"
    );
    assert_ne!(
        theme::grid::current_match().bg,
        theme::grid::search_match().bg,
        "the two match styles are visually distinct"
    );
}

#[test]
fn cell_scoped_match_does_not_straddle_the_column_gutter() {
    // Two adjacent single-char cells "a" | "a" render as "a  a" (2-space gutter). A needle that
    // could only match across the gutter ("a a") must NOT highlight — matches are cell-scoped.
    let t = Table::new(vec![
        Column::new(
            "x",
            crate::schema::ColumnType::Text,
            vec![Cell::Text("a".into())],
        ),
        Column::new(
            "y",
            crate::schema::ColumnType::Text,
            vec![Cell::Text("a".into())],
        ),
    ]);
    let frame = layout_grid(&t, &GridView::new(80, 24));
    let line = style_body_line(&frame.body[0], Modifier::empty(), "a a", false);
    assert!(
        line.spans.iter().all(|s| !is_match_styled(s.style)),
        "a needle spanning the gutter must not match"
    );
}

#[test]
fn search_highlight_covers_every_occurrence_in_the_line() {
    let t = Table::new(vec![
        Column::new(
            "a",
            crate::schema::ColumnType::Text,
            vec![Cell::Text("go".into())],
        ),
        Column::new(
            "b",
            crate::schema::ColumnType::Text,
            vec![Cell::Text("GOing".into())],
        ),
    ]);
    let frame = layout_grid(&t, &GridView::new(80, 24));
    let line = style_body_line(&frame.body[0], Modifier::empty(), "go", false);
    let matched: Vec<&str> = line
        .spans
        .iter()
        .filter(|s| is_match_styled(s.style))
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(matched, vec!["go", "GO"], "both cells' runs highlight");
}

#[test]
fn null_glyph_keeps_null_style_even_when_needle_says_null() {
    // The filter says NULL never matches; the render agrees — a needle of "null" must not
    // repaint the absent-value glyph as a match.
    let t = Table::new(vec![
        Column::new("a", crate::schema::ColumnType::Text, vec![Cell::Null]),
        Column::new(
            "b",
            crate::schema::ColumnType::Text,
            vec![Cell::Text("nullable".into())],
        ),
    ]);
    let frame = layout_grid(&t, &GridView::new(80, 24));
    let line = style_body_line(&frame.body[0], Modifier::empty(), "null", false);
    let matched: String = line
        .spans
        .iter()
        .filter(|s| is_match_styled(s.style))
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(matched, "null", "only column b's literal text highlights");
    let dimmed: String = line
        .spans
        .iter()
        .filter(|s| is_null_styled(s.style))
        .map(|s| s.content.as_ref())
        .collect();
    assert!(dimmed.contains("NULL"), "the genuine null stays dimmed");
}

#[test]
fn empty_needle_leaves_the_line_unstyled() {
    let t = typed_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    let line = style_body_line(&frame.body[0], Modifier::empty(), "", false);
    assert!(line.spans.iter().all(|s| !is_match_styled(s.style)));
}

#[test]
fn stale_dim_rides_match_highlight_spans() {
    let t = typed_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    let line = style_body_line(&frame.body[0], Modifier::DIM, "ada", false);
    for span in &line.spans {
        assert!(
            span.style.add_modifier.contains(Modifier::DIM),
            "every span (matched or not) carries the stale dim"
        );
    }
}

#[test]
fn render_grid_paints_needle_matches_into_the_buffer() {
    let t = typed_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    let backend = TestBackend::new(80, 10);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            render_grid(
                f,
                Rect::new(0, 0, 80, 10),
                &frame,
                GridPaint {
                    accent: theme::base::CYAN,
                    search_needle: "ada",
                    current_match_row: None,
                    ..Default::default()
                },
            )
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    // Find the "Ada" cell in body row 0 (screen row 1) and check its background.
    let row_text: String = (0..80).map(|x| buf[(x, 1)].symbol().to_string()).collect();
    let col = row_text.find("Ada").expect("Ada on the first body row");
    assert_eq!(
        buf[(col as u16, 1)].style().bg,
        theme::grid::search_match().bg,
        "the matched run carries the highlight band"
    );
    assert_ne!(
        buf[(0, 1)].style().bg,
        theme::grid::search_match().bg,
        "unmatched cells keep the plain background"
    );
}

#[test]
fn hovered_row_carries_the_hover_background() {
    let t = typed_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            render_grid(
                f,
                Rect::new(0, 0, 80, 10),
                &frame,
                GridPaint {
                    hovered_row: Some(1),
                    accent: theme::base::CYAN,
                    current_match_row: None,
                    ..Default::default()
                },
            )
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    // Body row 1 sits on screen row 2 (header on row 0, body rows from row 1).
    // Column 0 of the hovered row carries the bright left accent bar; a later column carries the
    // background band.
    let bar_cell = &buf[(0, 2)];
    let band_cell = &buf[(3, 2)];
    let normal_cell = &buf[(3, 1)];
    assert_eq!(
        bar_cell.symbol(),
        "\u{258c}",
        "the hovered row gets the left bar"
    );
    assert_eq!(
        bar_cell.style().fg,
        theme::grid::hover_bar(theme::base::CYAN).fg,
        "the bar takes the pane accent color"
    );
    assert_eq!(
        band_cell.style().bg,
        theme::grid::hovered_bg().bg,
        "the hovered body row is painted with the hover band"
    );
    assert_ne!(
        normal_cell.style().bg,
        theme::grid::hovered_bg().bg,
        "non-hovered rows keep the plain background"
    );
}

#[test]
fn hovered_row_respects_the_scroll_offset() {
    let t = typed_table();
    let frame = layout_grid(&t, &GridView::new(80, 24));
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    // Scrolled down by 1: absolute row 2 is the second visible body row (screen row 2).
    terminal
        .draw(|f| {
            render_grid(
                f,
                Rect::new(0, 0, 80, 10),
                &frame,
                GridPaint {
                    v_row_offset: 1,
                    hovered_row: Some(2),
                    accent: theme::base::CYAN,
                    current_match_row: None,
                    ..Default::default()
                },
            )
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    assert_eq!(
        buf[(0, 2)].symbol(),
        "\u{258c}",
        "the bar follows the hovered row to its scrolled screen position"
    );
    assert_eq!(
        buf[(3, 2)].style().bg,
        theme::grid::hovered_bg().bg,
        "hover matches the absolute row index against the scrolled window"
    );
    assert_ne!(buf[(3, 1)].style().bg, theme::grid::hovered_bg().bg);
}
