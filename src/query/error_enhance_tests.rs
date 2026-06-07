//! Golden-table tests for DuckDB error -> friendly message.
//!
//! One row per known DuckDB error category (P2.10 + the P5.3 polish extensions). The raw strings
//! are real DuckDB 1.10503.1 phrasings (or close paraphrases); the assertions pin the exact
//! friendly one-liner so a regression in the mapping is caught.

use crate::query::error_enhance::{enhance, enhance_with_schema};
use crate::schema::{ColumnMeta, Schema, types::ColumnType};

#[test]
fn unknown_column() {
    let raw = "Binder Error: Referenced column \"reigon\" not found in FROM clause!";
    assert_eq!(enhance(raw), "unknown column: \"reigon\"");
}

#[test]
fn unknown_column_without_quoted_name_is_generic() {
    // "column ... not found" but no quoted identifier to pull → the generic message.
    let raw = "Binder Error: a referenced column was not found";
    assert_eq!(enhance(raw), "unknown column");
}

#[test]
fn unknown_table() {
    let raw = "Catalog Error: Table with name foo does not exist!";
    assert_eq!(enhance(raw), "unknown table (the loaded CSV is table `t`)");
}

#[test]
fn syntax_error_pins_token() {
    let raw = "Parser Error: syntax error at or near \"FROM\"";
    assert_eq!(enhance(raw), "syntax error near \"FROM\"");
}

#[test]
fn syntax_error_without_token_passes_through() {
    let raw = "Parser Error: syntax error at end of input";
    assert_eq!(enhance(raw), "syntax error: syntax error at end of input");
}

#[test]
fn conversion_error_names_value_and_target() {
    let raw = "Conversion Error: Could not convert string 'abc' to INT64";
    assert_eq!(enhance(raw), "type error: can't read 'abc' as INT64");
}

#[test]
fn conversion_error_fallback_passes_through() {
    let raw = "Conversion Error: type DECIMAL out of range";
    assert!(enhance(raw).starts_with("type error:"));
}

#[test]
fn conversion_error_with_value_but_no_target_passes_through() {
    // Has the `convert string '...'` value but no ` to <type>` tail → falls back to the plain
    // "type error:" lead-in (the rsplit_once None branch).
    let raw = "Conversion Error: Could not convert string 'abc' somehow";
    assert_eq!(
        enhance(raw),
        "type error: Could not convert string 'abc' somehow"
    );
}

#[test]
fn type_mismatch_is_type_error() {
    let raw = "Binder Error: No function matches the given name and argument types 'date_trunc(VARCHAR)'.";
    assert!(enhance(raw).starts_with("type error:"));
}

#[test]
fn unknown_function() {
    let raw = "Catalog Error: Scalar Function with name lowerr does not exist!";
    assert_eq!(enhance(raw), "unknown function: lowerr");
}

#[test]
fn ambiguous_column() {
    let raw = "Binder Error: Ambiguous reference to column name \"id\"";
    assert!(enhance(raw).starts_with("ambiguous column:"));
}

#[test]
fn division_by_zero() {
    let raw = "Out of Range Error: Division by zero!";
    assert_eq!(enhance(raw), "division by zero");
}

#[test]
fn aggregate_in_where() {
    let raw = "Binder Error: aggregate function calls cannot be used in the WHERE clause";
    assert_eq!(
        enhance(raw),
        "aggregates aren't allowed in WHERE (use HAVING)"
    );
}

#[test]
fn skips_line_and_caret_context() {
    let raw =
        "Parser Error: syntax error at end of input\nLINE 1: SELECT * FROM\n                     ^";
    assert_eq!(enhance(raw), "syntax error: syntax error at end of input");
}

#[test]
fn interrupt_is_graceful() {
    assert_eq!(enhance("INTERRUPT Error: interrupted"), "query cancelled");
}

#[test]
fn unrecognized_passes_through_cleaned() {
    assert_eq!(
        enhance("Error: something unusual happened"),
        "something unusual happened"
    );
}

#[test]
fn never_empty_never_panics() {
    let _ = enhance("");
    let _ = enhance("\n\n\n");
    let _ = enhance("LINE 1: x\n^");
    // arbitrary bytes / multibyte
    let _ = enhance("💥 weird \u{0} input");
}

// --- did-you-mean against the schema (enhance_with_schema) ---

fn schema() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("region", ColumnType::Text),
        ColumnMeta::new("amount", ColumnType::Float),
        ColumnMeta::new("created_at", ColumnType::Date),
    ])
}

#[test]
fn did_you_mean_suggests_near_column() {
    let raw = "Binder Error: Referenced column \"reigon\" not found in FROM clause!";
    assert_eq!(
        enhance_with_schema(raw, &schema()),
        "unknown column: \"reigon\" — did you mean \"region\"?"
    );
}

#[test]
fn did_you_mean_suggests_on_prefix_subsequence() {
    // A typed prefix/abbreviation ("amt") is a subsequence of "amount".
    let raw = "Binder Error: Referenced column \"amt\" not found in FROM clause!";
    assert_eq!(
        enhance_with_schema(raw, &schema()),
        "unknown column: \"amt\" — did you mean \"amount\"?"
    );
}

#[test]
fn did_you_mean_picks_the_closest_of_several_near_columns() {
    // Two plausible columns; the nearer (smaller edit distance) wins, deterministically.
    let s = Schema::new(vec![
        ColumnMeta::new("amount", ColumnType::Float),
        ColumnMeta::new("amounts", ColumnType::Float),
    ]);
    let raw = "Binder Error: Referenced column \"amont\" not found in FROM clause!";
    // "amont" -> "amount" (dist 1) beats "amounts" (dist 2).
    assert_eq!(
        enhance_with_schema(raw, &s),
        "unknown column: \"amont\" — did you mean \"amount\"?"
    );
}

#[test]
fn did_you_mean_omitted_when_nothing_close() {
    let raw = "Binder Error: Referenced column \"xyzzy\" not found in FROM clause!";
    assert_eq!(
        enhance_with_schema(raw, &schema()),
        "unknown column: \"xyzzy\""
    );
}

#[test]
fn schema_variant_still_handles_non_column_errors() {
    // A non-column error is unaffected by the schema variant.
    let raw = "Parser Error: syntax error at or near \"FROM\"";
    assert_eq!(
        enhance_with_schema(raw, &schema()),
        "syntax error near \"FROM\""
    );
}
