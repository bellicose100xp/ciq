//! Tests for the SELECT-pane column-picker state machine.
//!
//! Headless / pure: every assertion is over the in-memory `PaletteState` plus a fixture
//! [`Schema`]; the popup's bidirectional sync with the SELECT pane (the App layer) is tested in
//! `app/app_tests/palette_tests.rs`.

use std::collections::BTreeSet;

use super::{PaletteState, parse_select_list};
use crate::schema::{ColumnMeta, ColumnType, Schema};

// --- fixtures ---

fn five_col_schema() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("name", ColumnType::Text),
        ColumnMeta::new("region", ColumnType::Text),
        ColumnMeta::new("amount", ColumnType::Float),
        ColumnMeta::new("active", ColumnType::Bool),
    ])
}

fn make_state() -> PaletteState {
    PaletteState::from_schema(&five_col_schema())
}

fn checked_indices(s: &PaletteState) -> BTreeSet<usize> {
    s.checked_set().clone()
}

// --- open_with_select ---

#[test]
fn open_with_select_star_checks_every_column() {
    let mut s = make_state();
    s.open_with_select("*");
    assert_eq!(checked_indices(&s), (0..5).collect::<BTreeSet<_>>());
}

#[test]
fn open_with_select_empty_checks_every_column() {
    let mut s = make_state();
    s.open_with_select("");
    assert_eq!(checked_indices(&s), (0..5).collect::<BTreeSet<_>>());
}

#[test]
fn open_with_select_whitespace_only_checks_every_column() {
    let mut s = make_state();
    s.open_with_select("   ");
    assert_eq!(checked_indices(&s), (0..5).collect::<BTreeSet<_>>());
}

#[test]
fn open_with_select_subset_checks_only_named_columns() {
    let mut s = make_state();
    s.open_with_select("id, name");
    assert_eq!(checked_indices(&s), [0, 1].into_iter().collect());
}

#[test]
fn open_with_select_is_case_insensitive() {
    let mut s = make_state();
    s.open_with_select("ID, Name");
    assert_eq!(checked_indices(&s), [0, 1].into_iter().collect());
}

#[test]
fn open_with_select_strips_quoted_idents() {
    let schema = Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("order", ColumnType::Int),
        ColumnMeta::new("region", ColumnType::Text),
    ]);
    let mut s = PaletteState::from_schema(&schema);
    s.open_with_select("\"order\", id");
    assert_eq!(checked_indices(&s), [0, 1].into_iter().collect());
}

#[test]
fn open_with_select_silently_drops_unknown_columns() {
    let mut s = make_state();
    s.open_with_select("id, nonexistent_col, name");
    assert_eq!(checked_indices(&s), [0, 1].into_iter().collect());
}

#[test]
fn open_with_select_resets_cursor_to_top() {
    let mut s = make_state();
    s.set_cursor(3);
    s.open_with_select("id");
    assert_eq!(s.cursor(), 0);
}

// --- cursor (bounded; no wrap) ---

#[test]
fn cursor_down_advances_then_stops_at_last_row() {
    let mut s = make_state();
    s.cursor_down();
    assert_eq!(s.cursor(), 1);
    for _ in 0..10 {
        s.cursor_down();
    }
    assert_eq!(s.cursor(), 4, "stops at the last row, no wrap");
}

#[test]
fn cursor_up_retreats_then_stops_at_first_row() {
    let mut s = make_state();
    s.set_cursor(3);
    s.cursor_up();
    assert_eq!(s.cursor(), 2);
    for _ in 0..10 {
        s.cursor_up();
    }
    assert_eq!(s.cursor(), 0, "stops at the first row, no wrap");
}

#[test]
fn cursor_ops_are_noop_on_empty() {
    let mut s = PaletteState::new(Vec::new());
    s.cursor_down();
    s.cursor_up();
    s.set_cursor(99);
    assert_eq!(s.cursor(), 0);
}

// --- toggle ---

#[test]
fn toggle_at_cursor_flips_only_that_index() {
    let mut s = make_state();
    s.set_cursor(2);
    s.toggle_at_cursor();
    assert_eq!(checked_indices(&s), [2].into_iter().collect());
    s.toggle_at_cursor();
    assert!(checked_indices(&s).is_empty());
}

#[test]
fn toggle_out_of_range_is_noop() {
    let mut s = make_state();
    s.toggle(99);
    assert!(checked_indices(&s).is_empty());
}

// --- bulk ops ---

#[test]
fn select_all_checks_every_index() {
    let mut s = make_state();
    s.select_all();
    assert_eq!(checked_indices(&s), (0..5).collect::<BTreeSet<_>>());
}

#[test]
fn deselect_all_clears_the_set() {
    let mut s = make_state();
    s.select_all();
    s.deselect_all();
    assert!(checked_indices(&s).is_empty());
}

#[test]
fn invert_complements_the_set() {
    let mut s = make_state();
    s.toggle(0);
    s.toggle(2);
    // checked = {0, 2}; invert -> {1, 3, 4}
    s.invert();
    assert_eq!(checked_indices(&s), [1, 3, 4].into_iter().collect());
}

#[test]
fn invert_is_self_inverse() {
    let mut s = make_state();
    s.toggle(1);
    s.toggle(3);
    let before = checked_indices(&s);
    s.invert();
    s.invert();
    assert_eq!(checked_indices(&s), before);
}

// --- write_to_select ---

#[test]
fn write_to_select_all_checked_emits_star() {
    let mut s = make_state();
    s.select_all();
    assert_eq!(s.write_to_select(), "*");
}

#[test]
fn write_to_select_subset_emits_comma_list_in_schema_order() {
    let mut s = make_state();
    // Toggle in a non-schema order; emission must be schema order regardless.
    s.toggle(3); // amount
    s.toggle(0); // id
    s.toggle(1); // name
    assert_eq!(s.write_to_select(), "id, name, amount");
}

#[test]
fn write_to_select_empty_emits_empty_string() {
    let s = make_state();
    assert_eq!(s.write_to_select(), "");
}

#[test]
fn write_to_select_quotes_keyword_collisions() {
    let schema = Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("order", ColumnType::Int),
    ]);
    let mut s = PaletteState::from_schema(&schema);
    s.toggle(1);
    assert_eq!(s.write_to_select(), "\"order\"");
}

#[test]
fn write_to_select_quotes_special_char_names() {
    let schema = Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("Total ($)", ColumnType::Float),
    ]);
    let mut s = PaletteState::from_schema(&schema);
    s.toggle(1);
    assert_eq!(s.write_to_select(), "\"Total ($)\"");
}

// --- parse_select_list helper ---

#[test]
fn parse_select_list_splits_top_level_commas() {
    let names = parse_select_list("id, name, region");
    assert_eq!(names, vec!["id", "name", "region"]);
}

#[test]
fn parse_select_list_strips_outer_quotes() {
    let names = parse_select_list("\"order\", id");
    assert_eq!(names, vec!["order", "id"]);
}

#[test]
fn parse_select_list_trims_whitespace() {
    let names = parse_select_list("  id  ,name ,  region");
    assert_eq!(names, vec!["id", "name", "region"]);
}

#[test]
fn parse_select_list_unescapes_doubled_quotes_in_quoted_idents() {
    let names = parse_select_list("\"we\"\"ird\"");
    assert_eq!(names, vec!["we\"ird"]);
}

#[test]
fn parse_select_list_does_not_split_inside_parens() {
    // `count(a, b)` is not a known schema column, so the App will silently drop it; this just
    // pins that the splitter does not split on the inner commas.
    let names = parse_select_list("id, count(a, b), name");
    assert_eq!(names.len(), 3);
    assert_eq!(names[0], "id");
    assert!(names[1].starts_with("count("));
    assert_eq!(names[2], "name");
}

#[test]
fn parse_select_list_empty_input_is_empty() {
    assert!(parse_select_list("").is_empty());
    assert!(parse_select_list("   ").is_empty());
}
