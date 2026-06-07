//! Tests for the shared SQL identifier quoting (`sql_ident.rs`).

use super::{is_reserved_keyword, needs_quoting, quote_ident, quote_ident_if_needed};

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
    assert_eq!(quote_ident_if_needed("*"), "*");
}

#[test]
fn quote_if_needed_quotes_reserved_and_special() {
    assert_eq!(quote_ident_if_needed("order"), "\"order\"");
    assert_eq!(quote_ident_if_needed("first name"), "\"first name\"");
    assert_eq!(quote_ident_if_needed("2col"), "\"2col\"");
    assert_eq!(quote_ident_if_needed(""), "\"\"");
    assert_eq!(quote_ident_if_needed("we\"ird"), "\"we\"\"ird\"");
}
