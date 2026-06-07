//! Tests for the palette state machine (`dev/PLAN.md` §6.2, `dev/DECISIONS.md` D3).
//!
//! Plain-assert unit tests, no terminal: the pure toggle/reorder/filter/predicate transitions and
//! the ownership byte-compare. The pure-core hard floor lives on this module, so every branch is a
//! real behavior case.

use super::*;
use crate::schema::{ColumnMeta, ColumnType, Schema};

fn sample() -> PaletteState {
    PaletteState::new(vec![
        ColumnRef::new("id", ColumnType::Int),
        ColumnRef::new("name", ColumnType::Text),
        ColumnRef::new("amount", ColumnType::Float),
        ColumnRef::new("created_at", ColumnType::Date),
    ])
}

// ── construction ──────────────────────────────────────────────────────────────────────────────

#[test]
fn new_starts_empty() {
    let s = sample();
    assert_eq!(s.all_columns().len(), 4);
    assert!(s.checked().is_empty());
    assert!(s.predicates().is_empty());
    assert_eq!(s.needle(), "");
    assert_eq!(s.cursor(), 0);
    assert_eq!(s.last_emitted(), None);
}

#[test]
fn from_schema_snapshots_names_and_types() {
    let schema = Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("city", ColumnType::Text),
    ]);
    let s = PaletteState::from_schema(&schema);
    assert_eq!(s.all_columns().len(), 2);
    assert_eq!(s.all_columns()[0], ColumnRef::new("id", ColumnType::Int));
    assert_eq!(s.all_columns()[1], ColumnRef::new("city", ColumnType::Text));
}

// ── toggle / selection order ──────────────────────────────────────────────────────────────────

#[test]
fn toggle_checks_in_selection_order() {
    let mut s = sample();
    s.toggle(2); // amount
    s.toggle(0); // id
    s.toggle(1); // name
    // Selection order is insertion order, not table order.
    assert_eq!(s.checked(), &[2, 0, 1]);
    assert!(s.is_checked(0));
    assert!(s.is_checked(1));
    assert!(s.is_checked(2));
    assert!(!s.is_checked(3));
}

#[test]
fn toggle_off_removes_preserving_order() {
    let mut s = sample();
    s.toggle(0);
    s.toggle(1);
    s.toggle(2);
    s.toggle(1); // uncheck the middle one
    assert_eq!(s.checked(), &[0, 2]);
    assert!(!s.is_checked(1));
}

#[test]
fn toggle_is_unique_no_duplicate_push() {
    // The ordered-unique invariant: toggling on then on again removes (it's a toggle), never dups.
    let mut s = sample();
    s.toggle(0);
    assert_eq!(s.checked(), &[0]);
    s.toggle(0);
    assert_eq!(s.checked(), &[] as &[usize]);
}

#[test]
fn toggle_out_of_range_is_ignored() {
    let mut s = sample();
    s.toggle(99);
    assert!(s.checked().is_empty());
}

#[test]
fn checked_columns_resolves_in_selection_order() {
    let mut s = sample();
    s.toggle(3); // created_at
    s.toggle(0); // id
    let names: Vec<&str> = s
        .checked_columns()
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, vec!["created_at", "id"]);
}

#[test]
fn toggle_cursor_toggles_the_highlighted_column() {
    let mut s = sample();
    s.cursor_down(); // cursor on index 1 (name)
    s.toggle_cursor();
    assert_eq!(s.checked(), &[1]);
}

#[test]
fn toggle_cursor_on_empty_filter_is_noop() {
    let mut s = sample();
    s.set_needle("zzz"); // matches nothing
    s.toggle_cursor();
    assert!(s.checked().is_empty());
}

// ── reorder ───────────────────────────────────────────────────────────────────────────────────

#[test]
fn move_selection_up_and_down() {
    let mut s = sample();
    s.toggle(0);
    s.toggle(1);
    s.toggle(2); // checked = [0,1,2]
    s.move_selection_down(0); // [1,0,2]
    assert_eq!(s.checked(), &[1, 0, 2]);
    s.move_selection_up(2); // [1,2,0]
    assert_eq!(s.checked(), &[1, 2, 0]);
}

#[test]
fn move_selection_at_ends_is_noop() {
    let mut s = sample();
    s.toggle(0);
    s.toggle(1); // [0,1]
    s.move_selection_up(0); // already first
    assert_eq!(s.checked(), &[0, 1]);
    s.move_selection_down(1); // already last
    assert_eq!(s.checked(), &[0, 1]);
}

#[test]
fn move_selection_of_unchecked_is_noop() {
    let mut s = sample();
    s.toggle(0); // [0]
    s.move_selection_up(3); // 3 not checked
    s.move_selection_down(3);
    assert_eq!(s.checked(), &[0]);
}

// ── fuzzy filter ──────────────────────────────────────────────────────────────────────────────

#[test]
fn empty_needle_matches_all_in_table_order() {
    let s = sample();
    assert_eq!(s.filtered_indices(), vec![0, 1, 2, 3]);
}

#[test]
fn needle_filters_by_subsequence_case_insensitive() {
    let mut s = sample();
    s.set_needle("am"); // "amount" matches; subsequence "am" also in "name"? n-a-m-e -> yes
    let idxs = s.filtered_indices();
    // Both "name" (n[a][m]e) and "amount" ([am]ount) contain "am" as a subsequence.
    assert!(idxs.contains(&1));
    assert!(idxs.contains(&2));
    // The order is preserved (table order), never reordered by the needle.
    assert_eq!(idxs, vec![1, 2]);
}

#[test]
fn needle_no_match_is_empty() {
    let mut s = sample();
    s.set_needle("zzz");
    assert!(s.filtered_indices().is_empty());
    assert_eq!(s.cursor_column_index(), None);
}

#[test]
fn push_and_pop_needle() {
    let mut s = sample();
    s.push_needle('i');
    s.push_needle('d');
    assert_eq!(s.needle(), "id");
    // "id" subsequence: id (i-d), created_at? c-r-e-a-t-e-[d]... no leading i. only "id".
    assert_eq!(s.filtered_indices(), vec![0]);
    s.pop_needle();
    assert_eq!(s.needle(), "i");
    s.pop_needle();
    assert_eq!(s.needle(), "");
}

// ── cursor navigation ─────────────────────────────────────────────────────────────────────────

#[test]
fn cursor_down_wraps() {
    let mut s = sample(); // 4 columns, cursor at 0
    s.cursor_down();
    assert_eq!(s.cursor(), 1);
    s.cursor_down();
    s.cursor_down();
    assert_eq!(s.cursor(), 3);
    s.cursor_down(); // wrap
    assert_eq!(s.cursor(), 0);
}

#[test]
fn cursor_up_wraps() {
    let mut s = sample();
    s.cursor_up(); // wrap to last
    assert_eq!(s.cursor(), 3);
    s.cursor_up();
    assert_eq!(s.cursor(), 2);
}

#[test]
fn cursor_navigation_on_empty_filter_is_noop() {
    let mut s = sample();
    s.set_needle("zzz");
    s.cursor_down();
    assert_eq!(s.cursor(), 0);
    s.cursor_up();
    assert_eq!(s.cursor(), 0);
}

#[test]
fn needle_edit_clamps_cursor_into_filtered_list() {
    let mut s = sample();
    s.cursor_down();
    s.cursor_down();
    s.cursor_down(); // cursor at 3
    assert_eq!(s.cursor(), 3);
    s.set_needle("id"); // filtered to one row; cursor must clamp to 0
    assert_eq!(s.cursor(), 0);
}

#[test]
fn cursor_column_index_maps_through_filter() {
    let mut s = sample();
    s.set_needle("am"); // filtered = [1, 2]
    assert_eq!(s.cursor_column_index(), Some(1));
    s.cursor_down();
    assert_eq!(s.cursor_column_index(), Some(2));
}

// ── predicates ────────────────────────────────────────────────────────────────────────────────

#[test]
fn add_and_remove_predicate() {
    let mut s = sample();
    s.add_predicate(Predicate::new(
        "name",
        ColumnType::Text,
        PredicateOp::Eq,
        "Acme",
    ));
    s.add_predicate(Predicate::null_test(
        "amount",
        ColumnType::Float,
        PredicateOp::Eq,
    ));
    assert_eq!(s.predicates().len(), 2);
    assert!(s.predicates()[1].is_null());
    s.remove_predicate(0);
    assert_eq!(s.predicates().len(), 1);
    assert!(s.predicates()[0].is_null());
}

#[test]
fn remove_predicate_out_of_range_is_noop() {
    let mut s = sample();
    s.add_predicate(Predicate::new("id", ColumnType::Int, PredicateOp::Gt, "5"));
    s.remove_predicate(9);
    assert_eq!(s.predicates().len(), 1);
}

#[test]
fn predicate_constructors_set_value_presence() {
    let v = Predicate::new("c", ColumnType::Text, PredicateOp::Like, "%a%");
    assert!(!v.is_null());
    assert_eq!(v.value.as_deref(), Some("%a%"));
    let n = Predicate::null_test("c", ColumnType::Text, PredicateOp::Neq);
    assert!(n.is_null());
    assert_eq!(n.value, None);
}

// ── ownership byte-compare (no parsing) ───────────────────────────────────────────────────────

#[test]
fn owns_is_a_byte_compare_against_last_emitted() {
    let mut s = sample();
    // Never emitted: owns nothing.
    assert!(!s.owns("SELECT * FROM t LIMIT 1000"));
    s.record_emitted("SELECT * FROM t LIMIT 1000");
    // Equal -> owned.
    assert!(s.owns("SELECT * FROM t LIMIT 1000"));
    // One byte different -> not owned (the user hand-typed).
    assert!(!s.owns("SELECT * FROM t LIMIT 999"));
    assert!(!s.owns("SELECT id FROM t LIMIT 1000"));
    assert_eq!(s.last_emitted(), Some("SELECT * FROM t LIMIT 1000"));
}

#[test]
fn column_ref_new_constructs() {
    let c = ColumnRef::new("x", ColumnType::Bool);
    assert_eq!(c.name, "x");
    assert_eq!(c.ty, ColumnType::Bool);
}
