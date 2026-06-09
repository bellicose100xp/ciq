//! Tests for the reused `Suggestion` model (`dev/PLAN.md` §5.1).
//!
//! Covers the constructors, the `with_*` chainers, the `field_type` slot carrying a `ColumnType`,
//! and the `SuggestionType` display labels (incl. the ciq-added `Keyword`/`Aggregate`).

use super::*;
use crate::schema::ColumnType;

#[test]
fn new_has_no_decorations() {
    let s = Suggestion::new("SELECT", SuggestionType::Keyword);
    assert_eq!(s.text, "SELECT");
    assert_eq!(s.suggestion_type, SuggestionType::Keyword);
    assert_eq!(s.description, None);
    assert_eq!(s.field_type, None);
    assert_eq!(s.signature, None);
}

#[test]
fn new_with_type_carries_column_type() {
    let s = Suggestion::new_with_type("created_at", SuggestionType::Field, Some(ColumnType::Date));
    assert_eq!(s.text, "created_at");
    assert_eq!(s.suggestion_type, SuggestionType::Field);
    assert_eq!(s.field_type, Some(ColumnType::Date));
}

#[test]
fn with_description_and_signature_chain() {
    let s = Suggestion::new("COUNT", SuggestionType::Aggregate)
        .with_signature("COUNT(expr)")
        .with_description("count of non-null rows");
    assert_eq!(s.signature.as_deref(), Some("COUNT(expr)"));
    assert_eq!(s.description.as_deref(), Some("count of non-null rows"));
}

#[test]
fn suggestion_type_display_labels() {
    assert_eq!(SuggestionType::Field.to_string(), "field");
    assert_eq!(SuggestionType::Function.to_string(), "function");
    assert_eq!(SuggestionType::Aggregate.to_string(), "aggregate");
    assert_eq!(SuggestionType::Operator.to_string(), "operator");
    assert_eq!(SuggestionType::Keyword.to_string(), "keyword");
    assert_eq!(SuggestionType::Value.to_string(), "value");
}

#[test]
fn suggestion_is_plain_data_clone_and_eq() {
    let a = Suggestion::new_with_type("amount", SuggestionType::Field, Some(ColumnType::Float));
    let b = a.clone();
    assert_eq!(a, b);
}

// --- AutocompleteState popup state machine (P3.6) ---

fn suggestions(names: &[&str]) -> Vec<Suggestion> {
    names
        .iter()
        .map(|n| Suggestion::new(*n, SuggestionType::Field))
        .collect()
}

#[test]
fn new_state_is_closed() {
    let s = AutocompleteState::new();
    assert!(!s.is_open());
    assert!(s.is_empty());
    assert_eq!(s.selected_suggestion(), None);
}

#[test]
fn open_with_candidates_opens_and_selects_first() {
    let mut s = AutocompleteState::new();
    s.open_with(suggestions(&["id", "status"]));
    assert!(s.is_open());
    assert_eq!(s.len(), 2);
    assert_eq!(s.selected(), 0);
    assert_eq!(s.selected_suggestion().map(|x| x.text.as_str()), Some("id"));
}

#[test]
fn open_with_empty_closes() {
    let mut s = AutocompleteState::new();
    s.open_with(suggestions(&["x"]));
    assert!(s.is_open());
    s.open_with(vec![]);
    assert!(!s.is_open(), "an empty candidate list closes the popup");
    assert_eq!(s.selected_suggestion(), None);
}

#[test]
fn close_clears_state() {
    let mut s = AutocompleteState::new();
    s.open_with(suggestions(&["a", "b"]));
    s.select_next();
    s.close();
    assert!(!s.is_open());
    assert!(s.is_empty());
    assert_eq!(s.selected(), 0);
}

#[test]
fn select_next_is_bounded_at_the_last_entry() {
    let mut s = AutocompleteState::new();
    s.open_with(suggestions(&["a", "b", "c"]));
    s.select_next();
    assert_eq!(s.selected(), 1);
    s.select_next();
    assert_eq!(s.selected(), 2);
    s.select_next();
    assert_eq!(s.selected(), 2, "no wrap; bounded at the last entry");
}

#[test]
fn select_prev_is_bounded_at_the_first_entry() {
    let mut s = AutocompleteState::new();
    s.open_with(suggestions(&["a", "b", "c"]));
    s.select_prev();
    assert_eq!(s.selected(), 0, "no wrap; bounded at 0");
    s.select_next();
    s.select_next();
    assert_eq!(s.selected(), 2);
    s.select_prev();
    assert_eq!(s.selected(), 1);
}

#[test]
fn selection_movement_is_noop_when_closed() {
    let mut s = AutocompleteState::new();
    s.select_next();
    s.select_prev();
    assert!(!s.is_open());
    assert_eq!(s.selected(), 0);
}

#[test]
fn reopen_resets_selection_to_first() {
    let mut s = AutocompleteState::new();
    s.open_with(suggestions(&["a", "b", "c"]));
    s.select_next();
    s.select_next();
    assert_eq!(s.selected(), 2);
    // A fresh recompute (next keystroke) re-opens at the first candidate.
    s.open_with(suggestions(&["x", "y"]));
    assert_eq!(s.selected(), 0);
}
