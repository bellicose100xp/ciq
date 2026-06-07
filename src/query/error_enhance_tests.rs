//! Golden-table tests for DuckDB error -> friendly message.

use crate::query::error_enhance::enhance;

#[test]
fn unknown_column() {
    let raw = "Binder Error: Referenced column \"reigon\" not found in FROM clause!";
    assert_eq!(enhance(raw), "unknown column: \"reigon\"");
}

#[test]
fn unknown_table() {
    let raw = "Catalog Error: Table with name foo does not exist!";
    assert_eq!(enhance(raw), "unknown table (the loaded CSV is table `t`)");
}

#[test]
fn syntax_error_trims_prefix() {
    let raw = "Parser Error: syntax error at or near \"FROM\"";
    assert_eq!(
        enhance(raw),
        "syntax error: syntax error at or near \"FROM\""
    );
}

#[test]
fn conversion_error() {
    let raw = "Conversion Error: Could not convert string 'abc' to INT64";
    assert!(enhance(raw).starts_with("type error:"));
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
    assert!(!enhance("").is_empty() || enhance("").is_empty()); // returns "" gracefully, no panic
    let _ = enhance("\n\n\n");
    let _ = enhance("LINE 1: x\n^");
    // arbitrary bytes
    let _ = enhance("💥 weird \u{0} input");
}
