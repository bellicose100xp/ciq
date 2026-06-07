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
