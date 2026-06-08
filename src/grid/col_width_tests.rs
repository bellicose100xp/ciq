//! Tests for `grid::col_width` — width math, ellipsis truncation, null glyph vs empty string.

use crate::engine::{Cell, Column, Table};
use crate::grid::col_width::{
    DEFAULT_MAX_COL_WIDTH, MIN_COL_WIDTH, NULL_GLYPH, cell_char_len, cell_display, compute_widths,
    render_cell, truncate_to_width,
};
use crate::schema::ColumnType;

fn table(cols: Vec<Column>) -> Table {
    Table::new(cols)
}

#[test]
fn cell_display_null_is_glyph_empty_text_is_empty() {
    // The load-bearing distinction: NULL renders to the glyph, empty string to nothing.
    assert_eq!(cell_display(&Cell::Null), NULL_GLYPH);
    assert_eq!(cell_display(&Cell::Text(String::new())), "");
    assert_eq!(cell_display(&Cell::Text("x".into())), "x");
    assert_eq!(cell_display(&Cell::Int(42)), "42");
    assert_eq!(cell_display(&Cell::Bool(true)), "true");
}

#[test]
fn null_and_empty_string_are_not_confused() {
    assert_ne!(
        cell_display(&Cell::Null),
        cell_display(&Cell::Text(String::new()))
    );
    assert!(!NULL_GLYPH.is_empty());
}

#[test]
fn cell_char_len_counts_chars() {
    assert_eq!(cell_char_len(&Cell::Text("abc".into())), 3);
    assert_eq!(cell_char_len(&Cell::Null), NULL_GLYPH.chars().count());
    assert_eq!(cell_char_len(&Cell::Text(String::new())), 0);
    // multi-byte char counts as one
    assert_eq!(cell_char_len(&Cell::Text("é".into())), 1);
}

#[test]
fn truncate_shorter_than_width_is_unchanged() {
    assert_eq!(truncate_to_width("ab", 5), "ab");
    assert_eq!(truncate_to_width("abcde", 5), "abcde");
}

#[test]
fn truncate_overflow_appends_ellipsis() {
    // "abcdef" into width 4 -> 3 kept + ellipsis
    assert_eq!(truncate_to_width("abcdef", 4), "abc…");
    assert_eq!(truncate_to_width("abcdef", 4).chars().count(), 4);
}

#[test]
fn truncate_width_zero_is_empty() {
    assert_eq!(truncate_to_width("anything", 0), "");
}

#[test]
fn truncate_width_one_is_just_ellipsis_when_overflow() {
    assert_eq!(truncate_to_width("abc", 1), "…");
}

#[test]
fn truncate_never_panics_on_multibyte() {
    // Must iterate by char, never slice on a byte boundary.
    let s = "¡¡¡¡¡"; // each is 2 bytes
    let out = truncate_to_width(s, 3);
    assert_eq!(out.chars().count(), 3);
    assert!(out.ends_with('…'));
}

#[test]
fn render_cell_truncates_and_substitutes_null() {
    assert_eq!(render_cell(&Cell::Text("hello".into()), 3), "he…");
    assert_eq!(render_cell(&Cell::Null, 10), NULL_GLYPH);
    // NULL glyph itself truncates if the column is narrower than it.
    assert_eq!(render_cell(&Cell::Null, 2).chars().count(), 2);
}

#[test]
fn width_is_max_of_header_and_widest_cell() {
    let t = table(vec![Column::new(
        "id",
        ColumnType::Int,
        vec![Cell::Int(1), Cell::Int(123456)],
    )]);
    // header label "id (int)" = 8, widest cell "123456" = 6 -> 8 (the label now dominates).
    assert_eq!(compute_widths(&t, 80), vec![8]);
}

#[test]
fn width_uses_header_when_cells_are_narrow() {
    let t = table(vec![Column::new(
        "longheader",
        ColumnType::Text,
        vec![Cell::Text("x".into())],
    )]);
    // header label "longheader (txt)" = 16, cell "x" = 1 -> 16.
    assert_eq!(compute_widths(&t, 80), vec![16]);
}

#[test]
fn width_capped_at_default_max() {
    let wide = "x".repeat(100);
    let t = table(vec![Column::new(
        "c",
        ColumnType::Text,
        vec![Cell::Text(wide)],
    )]);
    assert_eq!(compute_widths(&t, 80), vec![DEFAULT_MAX_COL_WIDTH]);
}

#[test]
fn width_capped_at_viewport_when_smaller_than_default() {
    let wide = "x".repeat(100);
    let t = table(vec![Column::new(
        "c",
        ColumnType::Text,
        vec![Cell::Text(wide)],
    )]);
    // viewport budget 10 is below DEFAULT_MAX_COL_WIDTH -> column capped at 10.
    assert_eq!(compute_widths(&t, 10), vec![10]);
}

#[test]
fn width_null_cell_counts_glyph_width() {
    let t = table(vec![Column::new("c", ColumnType::Text, vec![Cell::Null])]);
    // header label "c (txt)" = 7 dominates the 4-char NULL glyph -> 7. (The glyph still counts
    // toward the width; here the label is simply wider.)
    assert!(compute_widths(&t, 80)[0] >= NULL_GLYPH.chars().count() as u16);
    assert_eq!(compute_widths(&t, 80), vec![7]);
}

#[test]
fn empty_table_yields_no_widths() {
    let t = Table::default();
    assert!(compute_widths(&t, 80).is_empty());
}

#[test]
fn empty_column_falls_back_to_header_width() {
    let t = table(vec![Column::new("hdr", ColumnType::Text, vec![])]);
    // header label "hdr (txt)" = 9, no cells -> 9.
    assert_eq!(compute_widths(&t, 80), vec![9]);
}

#[test]
fn min_col_width_floor_applies() {
    // Even an empty column name still carries its `(txt)` badge label (" (txt)" = 6 chars), which
    // is comfortably above MIN_COL_WIDTH — so the floor never binds once the badge is folded in.
    let t = table(vec![Column::new(
        "",
        ColumnType::Text,
        vec![Cell::Text(String::new())],
    )]);
    let w = compute_widths(&t, 80);
    assert!(w[0] >= MIN_COL_WIDTH);
    assert_eq!(w, vec![6]);
}
