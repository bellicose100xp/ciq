//! Tests for `insertion` — replace-the-partial insert with SQL identifier/value quoting
//! (`dev/PLAN.md` §5.7, §5.6 round-trip property).
//!
//! Pure-core hard-floor module: every branch is a real behavior case (which span is replaced,
//! when a name is quoted, when a value is a string literal). Plus a `proptest` for the §5.6
//! invariant that the cursor always lands on a UTF-8 boundary and the op never panics.

use super::*;
use crate::schema::ColumnType;
use proptest::prelude::*;

fn field(name: &str) -> Suggestion {
    Suggestion::new(name, SuggestionType::Field)
}

fn typed_field(name: &str, ty: ColumnType) -> Suggestion {
    Suggestion::new_with_type(name, SuggestionType::Field, Some(ty))
}

fn keyword(name: &str) -> Suggestion {
    Suggestion::new(name, SuggestionType::Keyword)
}

fn value(text: &str, ty: ColumnType) -> Suggestion {
    Suggestion::new_with_type(text, SuggestionType::Value, Some(ty))
}

// --- partial replacement ---

#[test]
fn replaces_partial_ident_at_cursor() {
    // "SELECT sta|" -> insert `status` replaces the `sta` partial.
    let q = "SELECT sta";
    let (out, cur) = insert_suggestion(q, q.len(), &field("status"));
    assert_eq!(out, "SELECT status");
    assert_eq!(cur, out.len());
}

#[test]
fn inserts_at_fresh_position_replacing_nothing() {
    // Cursor after a space (no partial) -> pure insert.
    let q = "SELECT ";
    let (out, cur) = insert_suggestion(q, q.len(), &field("id"));
    assert_eq!(out, "SELECT id");
    assert_eq!(cur, out.len());
}

#[test]
fn preserves_text_to_the_right_of_cursor() {
    // Mid-query: "SELECT sta| FROM t" -> only the partial under the cursor is replaced.
    let q = "SELECT sta FROM t";
    let cursor = "SELECT sta".len();
    let (out, cur) = insert_suggestion(q, cursor, &field("status"));
    assert_eq!(out, "SELECT status FROM t");
    assert_eq!(&out[..cur], "SELECT status");
}

#[test]
fn keyword_suggestion_inserts_verbatim() {
    let q = "SELECT * FROM t WH";
    let (out, _) = insert_suggestion(q, q.len(), &keyword("WHERE"));
    assert_eq!(out, "SELECT * FROM t WHERE");
}

#[test]
fn star_field_inserts_unquoted() {
    let q = "SELECT ";
    let (out, _) = insert_suggestion(q, q.len(), &field("*"));
    assert_eq!(out, "SELECT *");
}

// --- identifier quoting (§5.7) ---

#[test]
fn keyword_collision_column_is_quoted() {
    // A column literally named `order` collides with a SQL keyword -> insert as `"order"`.
    let q = "SELECT or";
    let (out, _) = insert_suggestion(q, q.len(), &field("order"));
    assert_eq!(out, "SELECT \"order\"");
}

#[test]
fn name_with_space_is_quoted() {
    let q = "SELECT ";
    let (out, _) = insert_suggestion(q, q.len(), &field("first name"));
    assert_eq!(out, "SELECT \"first name\"");
}

#[test]
fn embedded_quote_is_doubled() {
    let q = "SELECT ";
    let (out, _) = insert_suggestion(q, q.len(), &field("we\"ird"));
    assert_eq!(out, "SELECT \"we\"\"ird\"");
}

#[test]
fn plain_identifier_is_not_quoted() {
    let q = "SELECT crea";
    let (out, _) = insert_suggestion(q, q.len(), &typed_field("created_at", ColumnType::Date));
    assert_eq!(out, "SELECT created_at");
}

#[test]
fn leading_digit_name_is_quoted() {
    let q = "SELECT ";
    let (out, _) = insert_suggestion(q, q.len(), &field("1col"));
    assert_eq!(out, "SELECT \"1col\"");
}

// --- value quoting ---

#[test]
fn text_value_inserts_as_string_literal_replacing_open_quote() {
    // "WHERE status = 'a|" -> the open literal (incl. its `'`) is replaced by `'active'`.
    let q = "WHERE status = 'a";
    let (out, cur) = insert_suggestion(q, q.len(), &value("active", ColumnType::Text));
    assert_eq!(out, "WHERE status = 'active'");
    assert_eq!(cur, out.len());
}

#[test]
fn text_value_at_empty_literal_inserts_string_literal() {
    let q = "WHERE status = '";
    let (out, _) = insert_suggestion(q, q.len(), &value("active", ColumnType::Text));
    assert_eq!(out, "WHERE status = 'active'");
}

#[test]
fn numeric_value_inserts_bare() {
    let q = "WHERE id = ";
    let (out, _) = insert_suggestion(q, q.len(), &value("42", ColumnType::Int));
    assert_eq!(out, "WHERE id = 42");
}

#[test]
fn value_with_embedded_apostrophe_is_escaped() {
    let q = "WHERE name = '";
    let (out, _) = insert_suggestion(q, q.len(), &value("O'Brien", ColumnType::Text));
    assert_eq!(out, "WHERE name = 'O''Brien'");
}

#[test]
fn finite_float_value_inserts_bare() {
    // An ordinary float value on a Float column stays a bare numeric literal.
    let q = "WHERE amount = ";
    let (out, _) = insert_suggestion(q, q.len(), &value("3.14", ColumnType::Float));
    assert_eq!(out, "WHERE amount = 3.14");
    // `-0` (from `f64::to_string` of -0.0) is a valid bare numeric literal too.
    let (out, _) = insert_suggestion(q, q.len(), &value("-0", ColumnType::Float));
    assert_eq!(out, "WHERE amount = -0");
}

#[test]
fn non_finite_float_value_is_quoted_not_bare() {
    // `inf`/`-inf`/`NaN` (as `f64::to_string` renders them) are NOT valid bare DuckDB literals —
    // they must fall back to a quoted literal that DuckDB casts against the DOUBLE column, so the
    // completed query stays valid instead of erroring with a "column not found".
    let q = "WHERE amount = ";
    let (out, _) = insert_suggestion(q, q.len(), &value("inf", ColumnType::Float));
    assert_eq!(out, "WHERE amount = 'inf'");
    let (out, _) = insert_suggestion(q, q.len(), &value("-inf", ColumnType::Float));
    assert_eq!(out, "WHERE amount = '-inf'");
    let (out, _) = insert_suggestion(q, q.len(), &value("NaN", ColumnType::Float));
    assert_eq!(out, "WHERE amount = 'NaN'");
}

// --- robustness ---

#[test]
fn cursor_past_end_is_clamped() {
    let q = "SELECT";
    let (out, cur) = insert_suggestion(q, 9999, &keyword("SELECT"));
    // The whole `SELECT` partial is replaced (cursor clamps to end, inside the token).
    assert_eq!(out, "SELECT");
    assert_eq!(cur, out.len());
}

#[test]
fn empty_query_pure_insert() {
    let (out, cur) = insert_suggestion("", 0, &keyword("SELECT"));
    assert_eq!(out, "SELECT");
    assert_eq!(cur, 6);
}

#[test]
fn utf8_text_to_the_right_is_preserved_and_boundary_safe() {
    // A multi-byte string literal sits to the right of the partial being completed; replacing the
    // partial must leave the multi-byte text intact and the cursor on a char boundary.
    let q = "SELECT sta FROM t WHERE city = 'Zürich'";
    let cursor = "SELECT sta".len();
    let (out, cur) = insert_suggestion(q, cursor, &field("status"));
    assert_eq!(out, "SELECT status FROM t WHERE city = 'Zürich'");
    assert!(out.is_char_boundary(cur));
    assert!(out.contains("Zürich"));
}

#[test]
fn mid_char_cursor_is_snapped_not_panicked() {
    // A cursor inside a multi-byte char (the §5.6 property allows arbitrary offsets) is snapped
    // to the boundary before it; insertion never panics.
    let q = "Zürich"; // 'ü' is two bytes (1..3)
    let (out, cur) = insert_suggestion(q, 2, &keyword("SELECT"));
    assert!(out.is_char_boundary(cur));
}

proptest! {
    /// §5.6 round-trip: for any query, cursor, and suggestion text, insertion never panics and the
    /// returned cursor is always a valid char boundary of the new text.
    #[test]
    fn insertion_never_panics_and_cursor_on_boundary(
        q in ".{0,40}",
        cursor in 0usize..50,
        text in "[a-zA-Z0-9_'\" ]{0,12}",
        kind_sel in 0u8..6,
    ) {
        let kind = match kind_sel {
            0 => SuggestionType::Field,
            1 => SuggestionType::Value,
            2 => SuggestionType::Keyword,
            3 => SuggestionType::Operator,
            4 => SuggestionType::Function,
            _ => SuggestionType::Aggregate,
        };
        let s = Suggestion::new_with_type(text, kind, Some(ColumnType::Text));
        let (out, cur) = insert_suggestion(&q, cursor, &s);
        prop_assert!(out.is_char_boundary(cur));
        prop_assert!(cur <= out.len());
    }
}
