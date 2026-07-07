//! Tests for the search state machine (open/close/confirm/needle edits) and the any-column row
//! filter (case-insensitive, NULL-never-matches, column order preserved).

use crate::engine::types::{Cell, Column, Table};
use crate::schema::ColumnType;

use super::{SearchState, filter_table, row_matches};

fn table() -> Table {
    Table::new(vec![
        Column::new(
            "id",
            ColumnType::Int,
            vec![Cell::Int(1), Cell::Int(2), Cell::Int(3)],
        ),
        Column::new(
            "region",
            ColumnType::Text,
            vec![
                Cell::Text("EU-WEST".into()),
                Cell::Text("NA".into()),
                Cell::Null,
            ],
        ),
        Column::new(
            "note",
            ColumnType::Text,
            vec![
                Cell::Text("alpha".into()),
                Cell::Text("beta eu".into()),
                Cell::Text("gamma".into()),
            ],
        ),
    ])
}

// --- state machine ---

#[test]
fn new_state_is_closed_and_empty() {
    let s = SearchState::new();
    assert!(!s.is_visible());
    assert!(!s.is_confirmed());
    assert!(!s.is_editing());
    assert!(!s.is_filtering());
    assert_eq!(s.needle(), "");
}

#[test]
fn open_enters_editing_mode() {
    let mut s = SearchState::new();
    s.open();
    assert!(s.is_visible());
    assert!(s.is_editing());
    assert!(!s.is_confirmed());
}

#[test]
fn push_pop_edit_the_needle() {
    let mut s = SearchState::new();
    s.open();
    s.push('e');
    s.push('u');
    assert_eq!(s.needle(), "eu");
    assert!(s.is_filtering());
    s.pop();
    assert_eq!(s.needle(), "e");
    s.pop();
    assert_eq!(s.needle(), "");
    assert!(!s.is_filtering(), "empty needle does not filter");
    s.pop(); // pop on empty is a no-op
    assert_eq!(s.needle(), "");
}

#[test]
fn confirm_freezes_editing_but_keeps_filtering() {
    let mut s = SearchState::new();
    s.open();
    s.push('x');
    s.confirm();
    assert!(s.is_confirmed());
    assert!(!s.is_editing());
    assert!(s.is_filtering(), "confirmed search still filters");
    s.unconfirm();
    assert!(s.is_editing());
}

#[test]
fn close_clears_everything() {
    let mut s = SearchState::new();
    s.open();
    s.push('x');
    s.confirm();
    s.close();
    assert!(!s.is_visible());
    assert!(!s.is_confirmed());
    assert_eq!(s.needle(), "");
    assert!(!s.is_filtering());
    // Reopening starts clean.
    s.open();
    assert_eq!(s.needle(), "");
}

// --- row filter ---

#[test]
fn row_matches_any_column_case_insensitive() {
    let t = table();
    assert!(row_matches(&t, 0, "eu"), "matches region EU-WEST");
    assert!(row_matches(&t, 1, "EU"), "matches note 'beta eu'");
    assert!(!row_matches(&t, 2, "eu"));
}

#[test]
fn row_matches_numeric_column_via_display_text() {
    let t = table();
    assert!(row_matches(&t, 1, "2"), "Int cell matches its digits");
    assert!(!row_matches(&t, 0, "2"));
}

#[test]
fn null_cell_never_matches_nonempty_needle() {
    let t = table();
    // Row 2's region is NULL; only its other columns can match.
    assert!(!row_matches(&t, 2, "null"), "NULL is absence, not text");
    assert!(row_matches(&t, 2, "gamma"));
}

#[test]
fn empty_needle_matches_every_row() {
    let t = table();
    for r in 0..t.row_count() {
        assert!(row_matches(&t, r, ""));
    }
}

#[test]
fn filter_table_keeps_matching_rows_in_order() {
    let t = table();
    let f = filter_table(&t, "eu");
    assert_eq!(f.row_count(), 2);
    // Original order preserved: row 0 (EU-WEST) then row 1 (beta eu).
    assert_eq!(f.columns()[0].cells, vec![Cell::Int(1), Cell::Int(2)]);
    assert_eq!(
        f.columns()[1].cells,
        vec![Cell::Text("EU-WEST".into()), Cell::Text("NA".into())]
    );
}

#[test]
fn filter_table_preserves_columns_when_nothing_matches() {
    let t = table();
    let f = filter_table(&t, "zzz");
    assert_eq!(f.row_count(), 0);
    assert_eq!(f.col_count(), 3);
    assert_eq!(f.columns()[1].name, "region");
    assert_eq!(f.columns()[1].ty, ColumnType::Text);
}

#[test]
fn filter_table_empty_needle_is_identity_shape() {
    let t = table();
    let f = filter_table(&t, "");
    assert_eq!(f.row_count(), t.row_count());
    assert_eq!(f.col_count(), t.col_count());
}
