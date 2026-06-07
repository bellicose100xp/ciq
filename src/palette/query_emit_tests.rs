//! Golden tests for the query emitter (`dev/PLAN.md` §6.2, `dev/DECISIONS.md` D3).
//!
//! The emitted byte format is a **stable identity surface** (the ownership check compares against
//! it), so these are exact-string goldens. They cover both quoting surfaces D3 calls out:
//! (a) identifier quoting in the projection, and (b) facet-predicate value quoting/escaping
//! (embedded quote, NULL, numeric vs string, dates). Reorder ordering is its own golden.

use super::*;
use crate::palette::palette_state::{ColumnRef, PaletteState, Predicate, PredicateOp};
use crate::schema::ColumnType;

fn state_with(cols: Vec<ColumnRef>) -> PaletteState {
    PaletteState::new(cols)
}

fn cols() -> Vec<ColumnRef> {
    vec![
        ColumnRef::new("id", ColumnType::Int),
        ColumnRef::new("name", ColumnType::Text),
        ColumnRef::new("amount", ColumnType::Float),
        ColumnRef::new("created_at", ColumnType::Date),
    ]
}

// ── projection ────────────────────────────────────────────────────────────────────────────────

#[test]
fn empty_selection_emits_select_star() {
    let s = state_with(cols());
    assert_eq!(emit(&s), "SELECT * FROM t LIMIT 1000");
}

#[test]
fn checked_columns_project_in_selection_order() {
    let mut s = state_with(cols());
    s.toggle(1); // name
    s.toggle(0); // id
    assert_eq!(emit(&s), "SELECT name, id FROM t LIMIT 1000");
}

#[test]
fn reorder_changes_projection_order() {
    // Reorder is its own exit criterion (§0/D3): the selection order drives the projection order.
    let mut s = state_with(cols());
    s.toggle(0); // id
    s.toggle(1); // name
    s.toggle(2); // amount  -> [id, name, amount]
    assert_eq!(emit(&s), "SELECT id, name, amount FROM t LIMIT 1000");
    s.move_selection_down(0); // [name, id, amount]
    assert_eq!(emit(&s), "SELECT name, id, amount FROM t LIMIT 1000");
    s.move_selection_up(2); // [name, amount, id]
    assert_eq!(emit(&s), "SELECT name, amount, id FROM t LIMIT 1000");
}

// ── (a) identifier quoting in the projection ──────────────────────────────────────────────────

#[test]
fn projection_quotes_reserved_word_and_special_chars() {
    let s = state_with(vec![
        ColumnRef::new("order", ColumnType::Int), // reserved word
        ColumnRef::new("Total ($)", ColumnType::Float), // spaces + special chars
        ColumnRef::new("plain", ColumnType::Text),
    ]);
    let mut s = s;
    s.toggle(0);
    s.toggle(1);
    s.toggle(2);
    assert_eq!(
        emit(&s),
        "SELECT \"order\", \"Total ($)\", plain FROM t LIMIT 1000"
    );
}

#[test]
fn projection_escapes_embedded_double_quote() {
    let mut s = state_with(vec![ColumnRef::new("we\"ird", ColumnType::Text)]);
    s.toggle(0);
    assert_eq!(emit(&s), "SELECT \"we\"\"ird\" FROM t LIMIT 1000");
}

#[test]
fn projection_quotes_a_column_literally_named_star() {
    // A column whose header is exactly `*` must project as the quoted literal `"*"`, NOT a bare `*`
    // that DuckDB would silently widen to all columns (the projection-contract divergence).
    let mut s = state_with(vec![
        ColumnRef::new("*", ColumnType::Text),
        ColumnRef::new("name", ColumnType::Text),
    ]);
    s.toggle(0); // check only the `*` column
    assert_eq!(emit(&s), "SELECT \"*\" FROM t LIMIT 1000");
}

// ── (b) facet-predicate value quoting/escaping ────────────────────────────────────────────────

#[test]
fn predicate_string_value_is_single_quoted() {
    let mut s = state_with(cols());
    s.add_predicate(Predicate::new(
        "name",
        ColumnType::Text,
        PredicateOp::Eq,
        "Acme",
    ));
    assert_eq!(emit(&s), "SELECT * FROM t WHERE name = 'Acme' LIMIT 1000");
}

#[test]
fn predicate_embedded_quote_is_doubled() {
    // The canonical D3 example: region = 'O''Brien'.
    let mut s = state_with(vec![ColumnRef::new("region", ColumnType::Text)]);
    s.add_predicate(Predicate::new(
        "region",
        ColumnType::Text,
        PredicateOp::Eq,
        "O'Brien",
    ));
    assert_eq!(
        emit(&s),
        "SELECT * FROM t WHERE region = 'O''Brien' LIMIT 1000"
    );
}

#[test]
fn predicate_numeric_value_is_bare() {
    let mut s = state_with(cols());
    s.add_predicate(Predicate::new("id", ColumnType::Int, PredicateOp::Gt, "5"));
    assert_eq!(emit(&s), "SELECT * FROM t WHERE id > 5 LIMIT 1000");
}

#[test]
fn predicate_float_value_is_bare() {
    let mut s = state_with(cols());
    s.add_predicate(Predicate::new(
        "amount",
        ColumnType::Float,
        PredicateOp::Le,
        "9.99",
    ));
    assert_eq!(emit(&s), "SELECT * FROM t WHERE amount <= 9.99 LIMIT 1000");
}

#[test]
fn predicate_bool_value_is_bare() {
    let mut s = state_with(vec![ColumnRef::new("active", ColumnType::Bool)]);
    s.add_predicate(Predicate::new(
        "active",
        ColumnType::Bool,
        PredicateOp::Eq,
        "true",
    ));
    assert_eq!(emit(&s), "SELECT * FROM t WHERE active = true LIMIT 1000");
}

#[test]
fn predicate_numeric_text_on_string_column_is_quoted() {
    // A value `5` on a TEXT column is a string '5', NOT a bare 5 (the numeric-vs-string surface).
    let mut s = state_with(vec![ColumnRef::new("code", ColumnType::Text)]);
    s.add_predicate(Predicate::new(
        "code",
        ColumnType::Text,
        PredicateOp::Eq,
        "5",
    ));
    assert_eq!(emit(&s), "SELECT * FROM t WHERE code = '5' LIMIT 1000");
}

#[test]
fn predicate_non_numeric_on_numeric_column_falls_back_to_quoted() {
    // A non-numeric value on a numeric column can't be bare (DuckDB would read it as an ident), so
    // it falls back to a quoted literal rather than injecting a bare token.
    let mut s = state_with(cols());
    s.add_predicate(Predicate::new(
        "amount",
        ColumnType::Float,
        PredicateOp::Eq,
        "NaN",
    ));
    assert_eq!(emit(&s), "SELECT * FROM t WHERE amount = 'NaN' LIMIT 1000");
}

#[test]
fn predicate_date_value_is_single_quoted() {
    let mut s = state_with(cols());
    s.add_predicate(Predicate::new(
        "created_at",
        ColumnType::Date,
        PredicateOp::Ge,
        "2024-01-01",
    ));
    assert_eq!(
        emit(&s),
        "SELECT * FROM t WHERE created_at >= '2024-01-01' LIMIT 1000"
    );
}

#[test]
fn predicate_null_test_is_is_null() {
    let mut s = state_with(cols());
    s.add_predicate(Predicate::null_test(
        "amount",
        ColumnType::Float,
        PredicateOp::Eq,
    ));
    assert_eq!(emit(&s), "SELECT * FROM t WHERE amount IS NULL LIMIT 1000");
}

#[test]
fn predicate_not_null_test_is_is_not_null() {
    let mut s = state_with(cols());
    s.add_predicate(Predicate::null_test(
        "name",
        ColumnType::Text,
        PredicateOp::Neq,
    ));
    assert_eq!(
        emit(&s),
        "SELECT * FROM t WHERE name IS NOT NULL LIMIT 1000"
    );
}

#[test]
fn predicate_like_renders_string_literal() {
    let mut s = state_with(cols());
    s.add_predicate(Predicate::new(
        "name",
        ColumnType::Text,
        PredicateOp::Like,
        "%acme%",
    ));
    assert_eq!(
        emit(&s),
        "SELECT * FROM t WHERE name LIKE '%acme%' LIMIT 1000"
    );
}

#[test]
fn predicate_like_on_numeric_column_quotes_the_pattern() {
    // A LIKE pattern is ALWAYS a string literal, regardless of column type — so `code LIKE 5` on a
    // numeric column emits `code LIKE '5'` (the intended string match), not a bare numeric `5`.
    let mut s = state_with(cols());
    s.add_predicate(Predicate::new(
        "id",
        ColumnType::Int,
        PredicateOp::Like,
        "5",
    ));
    assert_eq!(emit(&s), "SELECT * FROM t WHERE id LIKE '5' LIMIT 1000");
}

#[test]
fn predicate_neq_value_uses_bang_eq() {
    let mut s = state_with(cols());
    s.add_predicate(Predicate::new("id", ColumnType::Int, PredicateOp::Neq, "0"));
    assert_eq!(emit(&s), "SELECT * FROM t WHERE id != 0 LIMIT 1000");
}

#[test]
fn predicate_lt_op() {
    let mut s = state_with(cols());
    s.add_predicate(Predicate::new("id", ColumnType::Int, PredicateOp::Lt, "10"));
    assert_eq!(emit(&s), "SELECT * FROM t WHERE id < 10 LIMIT 1000");
}

// ── conjunction + projection together ─────────────────────────────────────────────────────────

#[test]
fn multiple_predicates_form_an_and_conjunction() {
    let mut s = state_with(cols());
    s.toggle(0); // id
    s.toggle(1); // name
    s.add_predicate(Predicate::new(
        "name",
        ColumnType::Text,
        PredicateOp::Eq,
        "Acme",
    ));
    s.add_predicate(Predicate::new(
        "amount",
        ColumnType::Float,
        PredicateOp::Gt,
        "100",
    ));
    assert_eq!(
        emit(&s),
        "SELECT id, name FROM t WHERE name = 'Acme' AND amount > 100 LIMIT 1000"
    );
}

#[test]
fn predicate_column_is_identifier_quoted() {
    // The predicate's column also goes through identifier quoting (a reserved-word column).
    let mut s = state_with(vec![ColumnRef::new("order", ColumnType::Int)]);
    s.add_predicate(Predicate::new(
        "order",
        ColumnType::Int,
        PredicateOp::Eq,
        "3",
    ));
    assert_eq!(emit(&s), "SELECT * FROM t WHERE \"order\" = 3 LIMIT 1000");
}

// ── limit ─────────────────────────────────────────────────────────────────────────────────────

#[test]
fn emit_with_explicit_limit() {
    let s = state_with(cols());
    assert_eq!(emit_with_limit(&s, 50), "SELECT * FROM t LIMIT 50");
}

#[test]
fn default_limit_matches_viewport_budget() {
    assert_eq!(DEFAULT_LIMIT, crate::app::VIEWPORT_ROW_LIMIT);
}
