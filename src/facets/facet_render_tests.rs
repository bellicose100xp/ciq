//! Tests for `facet_render` — the pure `format_facets` line builder, the histogram bar-width math,
//! and the popup blit (`insta` + `ratatui::TestBackend`, logical cells only) (P4.6, §6.5).
//!
//! The snapshots prove the *logical* content (stat lines, proportional bars). True-terminal glyphs,
//! bar color, and placement are the §4.7 human surface, NOT asserted here.

use super::*;
use crate::facets::facet_state::{FacetBar, FacetResult, FacetState};
use crate::schema::ColumnType;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

fn line_strings(lines: &[ratatui::text::Line]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect()
}

fn render(state: &FacetState, w: u16, h: u16, area: Rect) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).expect("TestBackend");
    t.draw(|f| render_facet(state, f, area)).expect("draw");
    t.backend().to_string()
}

// --- bar-length math (the §6.5 `bar_len = count * inner_width / max_count` formula) ---

#[test]
fn bar_length_scales_proportionally() {
    // The max-count bar fills the whole budget; half-count fills half.
    assert_eq!(bar_length(100, 100, 20), 20);
    assert_eq!(bar_length(50, 100, 20), 10);
    assert_eq!(bar_length(0, 100, 20), 0);
}

#[test]
fn bar_length_zero_max_is_safe() {
    // A zero max-count (summary / empty histogram) never divides by zero.
    assert_eq!(bar_length(5, 0, 20), 0);
    assert_eq!(bar_length(0, 0, 0), 0);
}

#[test]
fn bar_length_clamps_to_budget() {
    // A count above the max (can't happen, but defensive) clamps to the budget.
    assert_eq!(bar_length(200, 100, 10), 10);
}

// --- format_facets: summary ---

#[test]
fn summary_lines_show_all_four_stats() {
    let r = FacetResult::Summary {
        min: Some("1".into()),
        max: Some("99".into()),
        distinct: 42,
        nulls: 3,
    };
    let lines = format_facets(&r, 30);
    let texts = line_strings(&lines);
    assert_eq!(texts.len(), 4);
    assert!(texts[0].starts_with("min: 1"), "got {:?}", texts[0]);
    assert!(texts[1].starts_with("max: 99"), "got {:?}", texts[1]);
    assert!(texts[2].starts_with("distinct: 42"), "got {:?}", texts[2]);
    assert!(texts[3].starts_with("nulls: 3"), "got {:?}", texts[3]);
}

#[test]
fn summary_null_min_max_shows_null_marker() {
    let r = FacetResult::Summary {
        min: None,
        max: None,
        distinct: 0,
        nulls: 5,
    };
    let texts = line_strings(&format_facets(&r, 30));
    assert!(texts[0].contains("(null)"), "got {:?}", texts[0]);
    assert!(texts[1].contains("(null)"), "got {:?}", texts[1]);
}

#[test]
fn summary_lines_padded_to_width() {
    let r = FacetResult::Summary {
        min: Some("a".into()),
        max: Some("b".into()),
        distinct: 1,
        nulls: 0,
    };
    for line in line_strings(&format_facets(&r, 24)) {
        assert_eq!(line.chars().count(), 24, "line not padded: {line:?}");
    }
}

// --- format_facets: histogram ---

#[test]
fn histogram_lines_show_counts_and_proportional_bars() {
    let r = FacetResult::Histogram {
        bars: vec![FacetBar::new("active", 100), FacetBar::new("archived", 50)],
        distinct: 2,
        nulls: 4,
    };
    let lines = format_facets(&r, 40);
    let texts = line_strings(&lines);
    // distinct + nulls + 2 bars.
    assert_eq!(texts.len(), 4);
    assert!(texts[0].starts_with("distinct: 2"));
    assert!(texts[1].starts_with("nulls: 4"));
    // The two value rows carry the value, the count, and a `#` bar; the larger count's bar is longer.
    let bar0 = texts[2].matches('#').count();
    let bar1 = texts[3].matches('#').count();
    assert!(texts[2].contains("active"), "got {:?}", texts[2]);
    assert!(texts[2].contains("100"), "got {:?}", texts[2]);
    assert!(
        bar0 > bar1,
        "max-count bar must be longer: {bar0} vs {bar1}"
    );
    assert_eq!(bar1 * 2, bar0, "50% count => half the bar");
}

#[test]
fn empty_histogram_shows_only_stat_lines() {
    let r = FacetResult::Histogram {
        bars: vec![],
        distinct: 0,
        nulls: 0,
    };
    let texts = line_strings(&format_facets(&r, 30));
    assert_eq!(texts.len(), 2, "only distinct + nulls, no bars");
}

#[test]
fn histogram_long_value_truncates() {
    let r = FacetResult::Histogram {
        bars: vec![FacetBar::new("a_very_long_value_that_overflows", 10)],
        distinct: 1,
        nulls: 0,
    };
    let texts = line_strings(&format_facets(&r, 24));
    // No line exceeds the width.
    for line in &texts {
        assert!(line.chars().count() <= 24, "overflow: {line:?}");
    }
}

// --- render (blit) ---

#[test]
fn pending_facet_shows_computing() {
    let state = FacetState::pending("status", ColumnType::Text);
    let screen = render(&state, 40, 8, Rect::new(0, 0, 30, 6));
    assert!(screen.contains("computing"), "screen:\n{screen}");
    // The title carries the column + type badge.
    assert!(screen.contains("status"), "screen:\n{screen}");
    assert!(screen.contains("txt"), "screen:\n{screen}");
}

#[test]
fn rendered_summary_shows_stats() {
    let mut state = FacetState::pending("id", ColumnType::Int);
    state.apply_result(&summary_result_table());
    let screen = render(&state, 40, 10, Rect::new(0, 0, 30, 8));
    assert!(screen.contains("min"), "screen:\n{screen}");
    assert!(screen.contains("max"), "screen:\n{screen}");
    assert!(screen.contains("distinct"), "screen:\n{screen}");
}

#[test]
fn snapshot_facet_histogram_80x24() {
    // The headless 80x24 snapshot at the canonical terminal size: a populated text facet.
    let mut state = FacetState::pending("status", ColumnType::Text);
    state.apply_result(&histogram_result_table());
    let screen = render(&state, 80, 24, Rect::new(0, 1, 34, 10));
    insta::assert_snapshot!(screen);
}

#[test]
fn render_does_not_panic_on_degenerate_area() {
    let state = FacetState::pending("id", ColumnType::Int);
    for (w, h) in [(1u16, 1u16), (2, 2), (3, 1), (1, 3)] {
        let _ = render(&state, w.max(1), h.max(1), Rect::new(0, 0, w, h));
    }
}

#[test]
fn render_no_op_on_zero_area() {
    let state = FacetState::pending("id", ColumnType::Int);
    let screen = render(&state, 10, 3, Rect::new(0, 0, 0, 0));
    assert!(
        screen.chars().all(|c| c == ' ' || c == '\n' || c == '"'),
        "zero area must paint nothing, got:\n{screen}"
    );
}

// --- fixtures ---

use crate::engine::types::{Cell, Column, Table};

fn summary_result_table() -> Table {
    Table::new(vec![
        Column::new("mn", ColumnType::Int, vec![Cell::Int(1)]),
        Column::new("mx", ColumnType::Int, vec![Cell::Int(99)]),
        Column::new("distinct_count", ColumnType::Int, vec![Cell::Int(42)]),
        Column::new("null_count", ColumnType::Int, vec![Cell::Int(3)]),
    ])
}

fn histogram_result_table() -> Table {
    let rows = [("active", 100i64), ("archived", 60), ("pending", 20)];
    Table::new(vec![
        Column::new(
            "value",
            ColumnType::Text,
            rows.iter().map(|(v, _)| Cell::Text((*v).into())).collect(),
        ),
        Column::new(
            "n",
            ColumnType::Int,
            rows.iter().map(|(_, n)| Cell::Int(*n)).collect(),
        ),
        Column::new(
            "distinct_count",
            ColumnType::Int,
            rows.iter().map(|_| Cell::Int(3)).collect(),
        ),
        Column::new(
            "null_count",
            ColumnType::Int,
            rows.iter().map(|_| Cell::Int(4)).collect(),
        ),
    ])
}
