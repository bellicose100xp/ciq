//! Tests for the shared SQL identifier quoting (`sql_ident.rs`).

use super::{
    is_bare_literal, is_reserved_keyword, needs_quoting, quote_ident, quote_ident_if_needed,
    render_typed_value, single_quote_literal,
};
use crate::schema::ColumnType;

#[test]
fn quote_ident_wraps_and_doubles_embedded_quotes() {
    assert_eq!(quote_ident("plain"), "\"plain\"");
    assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
    assert_eq!(quote_ident("a\"\"b"), "\"a\"\"\"\"b\"");
    assert_eq!(quote_ident(""), "\"\"");
    assert_eq!(quote_ident("Total ($)"), "\"Total ($)\"");
}

#[test]
fn quote_ident_handles_unicode_verbatim() {
    // Non-ASCII passes through inside the quotes (not a `"`), exercising the char loop.
    assert_eq!(quote_ident("café"), "\"café\"");
}

#[test]
fn needs_quoting_recognises_bare_identifiers() {
    assert!(!needs_quoting("id"));
    assert!(!needs_quoting("user_id"));
    assert!(!needs_quoting("_private"));
    assert!(!needs_quoting("col2"));
}

#[test]
fn needs_quoting_flags_non_bare_names() {
    assert!(needs_quoting("")); // empty
    assert!(needs_quoting("2col")); // leading digit
    assert!(needs_quoting("first name")); // space
    assert!(needs_quoting("Total ($)")); // special chars
    assert!(needs_quoting("a-b")); // dash
}

#[test]
fn reserved_keyword_lookup_is_case_insensitive() {
    assert!(is_reserved_keyword("order"));
    assert!(is_reserved_keyword("ORDER"));
    assert!(is_reserved_keyword("Select"));
    assert!(is_reserved_keyword("from"));
    assert!(!is_reserved_keyword("amount"));
    assert!(!is_reserved_keyword("status"));
}

#[test]
fn quote_if_needed_leaves_bare_identifiers_untouched() {
    assert_eq!(quote_ident_if_needed("id"), "id");
    assert_eq!(quote_ident_if_needed("user_id"), "user_id");
}

#[test]
fn quote_if_needed_quotes_a_column_literally_named_star() {
    // A real column whose header is the single char `*` must be quoted to `"*"` (the literal
    // column), NOT left as a bare `*` that DuckDB would expand to the all-columns wildcard. The
    // emitter's own wildcard is a separate directly-built `*` literal, not routed through here.
    assert_eq!(quote_ident_if_needed("*"), "\"*\"");
}

#[test]
fn quote_if_needed_quotes_reserved_and_special() {
    assert_eq!(quote_ident_if_needed("order"), "\"order\"");
    assert_eq!(quote_ident_if_needed("first name"), "\"first name\"");
    assert_eq!(quote_ident_if_needed("2col"), "\"2col\"");
    assert_eq!(quote_ident_if_needed(""), "\"\"");
    assert_eq!(quote_ident_if_needed("we\"ird"), "\"we\"\"ird\"");
}

// ── single_quote_literal ──────────────────────────────────────────────────────────────────────

#[test]
fn single_quote_literal_doubles_embedded_quote() {
    assert_eq!(single_quote_literal("plain"), "'plain'");
    assert_eq!(single_quote_literal("O'Brien"), "'O''Brien'");
    assert_eq!(single_quote_literal(""), "''");
}

// ── is_bare_literal (the one shared bare-vs-quote predicate) ──────────────────────────────────

#[test]
fn is_bare_literal_accepts_booleans_and_finite_numbers() {
    assert!(is_bare_literal("true"));
    assert!(is_bare_literal("FALSE"));
    assert!(is_bare_literal("42"));
    assert!(is_bare_literal("3.14"));
    assert!(is_bare_literal("-0"));
    assert!(is_bare_literal("1e9"));
}

#[test]
fn is_bare_literal_rejects_text_empty_and_non_finite() {
    assert!(!is_bare_literal("hello"));
    assert!(!is_bare_literal(""));
    assert!(!is_bare_literal("0x1F"));
    assert!(!is_bare_literal("inf"));
    assert!(!is_bare_literal("-inf"));
    assert!(!is_bare_literal("NaN"));
}

// ── render_typed_value (shared by both emitters) ──────────────────────────────────────────────

#[test]
fn render_typed_value_bares_numeric_on_numeric_column() {
    assert_eq!(render_typed_value("5", Some(&ColumnType::Int)), "5");
    assert_eq!(render_typed_value("9.99", Some(&ColumnType::Float)), "9.99");
    assert_eq!(render_typed_value("true", Some(&ColumnType::Bool)), "true");
}

#[test]
fn render_typed_value_quotes_text_temporal_and_unknown() {
    assert_eq!(
        render_typed_value("Acme", Some(&ColumnType::Text)),
        "'Acme'"
    );
    assert_eq!(
        render_typed_value("2024-01-01", Some(&ColumnType::Date)),
        "'2024-01-01'"
    );
    // A bare-looking value on a TEXT column is a string ('5'), and a None type defers to quoting.
    assert_eq!(render_typed_value("5", Some(&ColumnType::Text)), "'5'");
    assert_eq!(render_typed_value("5", None), "'5'");
}

#[test]
fn render_typed_value_quotes_non_bare_on_numeric_column() {
    // The previously-divergent case: a non-numeric / non-finite value on a numeric column quotes,
    // never injects a bare identifier token.
    assert_eq!(
        render_typed_value("hello", Some(&ColumnType::Int)),
        "'hello'"
    );
    assert_eq!(render_typed_value("NaN", Some(&ColumnType::Float)), "'NaN'");
}
