//! Tests for the clause-context detector (`dev/PLAN.md` §5.3/§5.4/§5.7).
//!
//! Table-driven: one case family per §5.4 mapping row, one per §5.7 tricky case, plus the §5.6
//! never-panic property over every byte offset. The cursor is expressed as a byte offset into the
//! source; most cases put it at end-of-input (the live-typing case), but the mid-query case puts
//! it deliberately interior.

use super::*;
use crate::sql_lexer::tokenize;

/// Detect the context for `src` with the cursor at byte `cursor`.
fn ctx_at(src: &str, cursor: usize) -> CursorContext {
    let toks = tokenize(src);
    detect_context(src, &toks, cursor)
}

/// Detect the context with the cursor at end of input (the common live-typing position).
fn ctx(src: &str) -> CursorContext {
    ctx_at(src, src.len())
}

// ── §5.4 mapping rows ─────────────────────────────────────────────────────────────────────────

#[test]
fn select_list_after_select() {
    // `SELECT ` / `SELECT a, ` -> SelectList.
    assert_eq!(
        ctx("SELECT "),
        CursorContext::SelectList {
            partial: String::new()
        }
    );
    assert_eq!(
        ctx("SELECT na"),
        CursorContext::SelectList {
            partial: "na".into()
        }
    );
    assert_eq!(
        ctx("SELECT id, na"),
        CursorContext::SelectList {
            partial: "na".into()
        }
    );
}

#[test]
fn select_list_inside_aggregate_paren() {
    // `SELECT COUNT(` -> still SelectList (columns + `*`).
    assert_eq!(
        ctx("SELECT COUNT("),
        CursorContext::SelectList {
            partial: String::new()
        }
    );
    assert_eq!(
        ctx("SELECT SUM(amo"),
        CursorContext::SelectList {
            partial: "amo".into()
        }
    );
}

#[test]
fn from_table_after_from_and_join() {
    assert_eq!(
        ctx("SELECT * FROM "),
        CursorContext::FromTable {
            partial: String::new()
        }
    );
    assert_eq!(
        ctx("SELECT * FROM us"),
        CursorContext::FromTable {
            partial: "us".into()
        }
    );
    assert_eq!(
        ctx("SELECT * FROM a JOIN b"),
        CursorContext::FromTable {
            partial: "b".into()
        }
    );
}

#[test]
fn predicate_after_where_and_boolean_connectors() {
    assert_eq!(
        ctx("WHERE "),
        CursorContext::Predicate {
            partial: String::new()
        }
    );
    assert_eq!(
        ctx("WHERE st"),
        CursorContext::Predicate {
            partial: "st".into()
        }
    );
    assert_eq!(
        ctx("WHERE a = 1 AND "),
        CursorContext::Predicate {
            partial: String::new()
        }
    );
    assert_eq!(
        ctx("WHERE a = 1 OR st"),
        CursorContext::Predicate {
            partial: "st".into()
        }
    );
    assert_eq!(
        ctx("SELECT * FROM t HAVING "),
        CursorContext::Predicate {
            partial: String::new()
        }
    );
}

#[test]
fn comparison_op_after_a_predicate_column() {
    // `WHERE col ` (column then space) -> ComparisonOp with the column as the (informational) lhs.
    assert_eq!(
        ctx("WHERE status "),
        CursorContext::ComparisonOp {
            lhs_col: Some("status".into())
        }
    );
    assert_eq!(
        ctx("WHERE a = 1 AND amount "),
        CursorContext::ComparisonOp {
            lhs_col: Some("amount".into())
        }
    );
}

#[test]
fn column_value_after_eq_literal() {
    // `WHERE col = '` -> ColumnValue for that column.
    assert_eq!(
        ctx("WHERE status = '"),
        CursorContext::ColumnValue {
            col: "status".into(),
            kind: TriggerKind::Eq,
            partial: String::new(),
        }
    );
    assert_eq!(
        ctx("WHERE status = 'act"),
        CursorContext::ColumnValue {
            col: "status".into(),
            kind: TriggerKind::Eq,
            partial: "act".into(),
        }
    );
}

#[test]
fn group_and_order_list() {
    assert_eq!(
        ctx("SELECT * FROM t GROUP BY "),
        CursorContext::GroupOrderList {
            partial: String::new()
        }
    );
    assert_eq!(
        ctx("SELECT * FROM t ORDER BY am"),
        CursorContext::GroupOrderList {
            partial: "am".into()
        }
    );
    assert_eq!(
        ctx("SELECT * FROM t GROUP BY a, b"),
        CursorContext::GroupOrderList {
            partial: "b".into()
        }
    );
}

#[test]
fn keyword_at_start_and_bare_position() {
    assert_eq!(
        ctx(""),
        CursorContext::Keyword {
            partial: String::new()
        }
    );
    assert_eq!(
        ctx("SEL"),
        CursorContext::Keyword {
            partial: "SEL".into()
        }
    );
}

// ── §5.7 tricky cases ─────────────────────────────────────────────────────────────────────────

#[test]
fn tricky_quoted_identifier_partial() {
    // `SELECT "ord` -> SelectList, partial is the inner text sans the leading quote.
    assert_eq!(
        ctx("SELECT \"ord"),
        CursorContext::SelectList {
            partial: "ord".into()
        }
    );
}

#[test]
fn tricky_qualified_name_strips_qualifier_and_is_one_column() {
    // `WHERE t.cre` -> Predicate; the detector must NOT read `t.cre` as a value or split it oddly.
    // partial is the trailing token text after the dot.
    assert_eq!(
        ctx("WHERE t.cre"),
        CursorContext::Predicate {
            partial: "cre".into()
        }
    );
    // `WHERE t.created_at = '` -> ColumnValue keyed on the BARE column name (qualifier stripped).
    assert_eq!(
        ctx("WHERE t.created_at = '2"),
        CursorContext::ColumnValue {
            col: "created_at".into(),
            kind: TriggerKind::Eq,
            partial: "2".into(),
        }
    );
}

#[test]
fn tricky_partial_vs_fresh_position() {
    // Fresh: empty partial lists all columns; partial filters.
    assert_eq!(
        ctx("WHERE "),
        CursorContext::Predicate {
            partial: String::new()
        }
    );
    assert_eq!(
        ctx("WHERE st"),
        CursorContext::Predicate {
            partial: "st".into()
        }
    );
}

#[test]
fn tricky_mid_query_edit_classifies_from_cursor_not_end() {
    // `SELECT a, |b FROM t` — cursor BETWEEN the comma+space and `b`. Classifies from the token
    // under the cursor (SelectList), not from end-of-string (which is FROM territory).
    let src = "SELECT a, b FROM t";
    let cursor = "SELECT a, ".len(); // just before `b`
    assert_eq!(
        ctx_at(src, cursor),
        CursorContext::SelectList {
            partial: String::new()
        }
    );
    // And one char into `b`:
    let cursor2 = "SELECT a, b".len();
    // end-of-`b` extends `b` in the select list.
    assert_eq!(
        ctx_at(src, cursor2),
        CursorContext::SelectList {
            partial: "b".into()
        }
    );
}

#[test]
fn tricky_unclosed_string_is_value_mode() {
    // `WHERE city = 'New` -> ColumnValue, partial `New`.
    assert_eq!(
        ctx("WHERE city = 'New"),
        CursorContext::ColumnValue {
            col: "city".into(),
            kind: TriggerKind::Eq,
            partial: "New".into(),
        }
    );
}

#[test]
fn tricky_in_list_stays_column_value() {
    // `WHERE status IN ('a', '` -> still ColumnValue for `status`.
    assert_eq!(
        ctx("WHERE status IN ('a', '"),
        CursorContext::ColumnValue {
            col: "status".into(),
            kind: TriggerKind::In,
            partial: String::new(),
        }
    );
    // first element too: `WHERE status IN ('`
    assert_eq!(
        ctx("WHERE status IN ('"),
        CursorContext::ColumnValue {
            col: "status".into(),
            kind: TriggerKind::In,
            partial: String::new(),
        }
    );
}

#[test]
fn tricky_like_is_value_mode_not_operator_mode() {
    // Deliberate dialect choice (inverse of jiq): LIKE -> offer distinct VALUES, not operators.
    assert_eq!(
        ctx("WHERE name LIKE '"),
        CursorContext::ColumnValue {
            col: "name".into(),
            kind: TriggerKind::Like,
            partial: String::new(),
        }
    );
    assert_eq!(
        ctx("WHERE name LIKE 'Ab"),
        CursorContext::ColumnValue {
            col: "name".into(),
            kind: TriggerKind::Like,
            partial: "Ab".into(),
        }
    );
}

#[test]
fn tricky_function_wrapping_column_inside_call() {
    // `WHERE lower(ci` -> the column position INSIDE the call is still a predicate column.
    assert_eq!(
        ctx("WHERE lower(ci"),
        CursorContext::Predicate {
            partial: "ci".into()
        }
    );
    // `SELECT date_trunc('day', ` -> SelectList column position for the second arg.
    assert_eq!(
        ctx("SELECT date_trunc('day', "),
        CursorContext::SelectList {
            partial: String::new()
        }
    );
}

#[test]
fn neq_and_cmp_trigger_kinds() {
    assert!(matches!(
        ctx("WHERE a != '"),
        CursorContext::ColumnValue {
            kind: TriggerKind::Neq,
            ..
        }
    ));
    assert!(matches!(
        ctx("WHERE a <> '"),
        CursorContext::ColumnValue {
            kind: TriggerKind::Neq,
            ..
        }
    ));
    assert!(matches!(
        ctx("WHERE a >= '"),
        CursorContext::ColumnValue {
            kind: TriggerKind::Cmp,
            ..
        }
    ));
}

#[test]
fn closed_literal_is_not_value_mode() {
    // After a complete value, the cursor is no longer in value mode — `WHERE a = 'x' |` returns to
    // a predicate-connector keyword position (no open literal).
    let src = "WHERE a = 'x' ";
    // Not a ColumnValue; it falls through to a keyword/predicate-ish position. We only assert it's
    // NOT mistakenly a ColumnValue.
    assert!(!matches!(ctx(src), CursorContext::ColumnValue { .. }));
}

// ── branch coverage: quoted-ident value columns, operator-ish predicate positions, scan-through ──

#[test]
fn quoted_ident_column_in_value_position_is_unquoted() {
    // A column literally named `order` (a reserved word) written as `"order"` resolves to the bare
    // `order` in value mode — exercises the QuotedIdent column-name path + `unquote_ident`.
    assert_eq!(
        ctx("WHERE \"order\" = 'a"),
        CursorContext::ColumnValue {
            col: "order".into(),
            kind: TriggerKind::Eq,
            partial: "a".into(),
        }
    );
    // The `""` escape inside a quoted ident unwraps to a single `"` in the resolved name.
    assert_eq!(
        ctx("WHERE \"we\"\"ird\" = '"),
        CursorContext::ColumnValue {
            col: "we\"ird".into(),
            kind: TriggerKind::Eq,
            partial: String::new(),
        }
    );
}

#[test]
fn quoted_ident_as_comparison_lhs() {
    // `WHERE "order" ` -> ComparisonOp; the (informational) lhs is the unquoted column name.
    assert_eq!(
        ctx("WHERE \"order\" "),
        CursorContext::ComparisonOp {
            lhs_col: Some("order".into())
        }
    );
}

#[test]
fn between_after_column_is_not_a_value_literal_yet() {
    // `WHERE a BETWEEN ` — BETWEEN is an operator-ish keyword, so the column position is already
    // past the LHS (exercises the LIKE/IN/IS/BETWEEN early-out in `is_predicate_lhs_position`).
    // No open literal, so this is NOT ColumnValue; we only assert it isn't misread as one.
    assert!(!matches!(
        ctx("WHERE a BETWEEN "),
        CursorContext::ColumnValue { .. }
    ));
}

#[test]
fn scan_passes_through_non_governing_keywords_to_the_clause() {
    // `ORDER BY a DESC, ` — the trailing `DESC` and the comma must not stop the backward walk; it
    // reaches `ORDER ... BY` and reports a GroupOrderList column position (exercises the
    // keep-scanning `_`/asc/desc keyword arm).
    assert_eq!(
        ctx("SELECT * FROM t ORDER BY a DESC, "),
        CursorContext::GroupOrderList {
            partial: String::new()
        }
    );
    // `SELECT a AS x, ` — `AS` is transparent; the walk reaches `SELECT`.
    assert_eq!(
        ctx("SELECT a AS x, "),
        CursorContext::SelectList {
            partial: String::new()
        }
    );
}

#[test]
fn bare_by_without_group_or_order_is_not_group_order_list() {
    // A `BY` whose preceding token is neither GROUP nor ORDER does not open a column position; the
    // scan continues left (exercises the `by` -> None branch). `LIMIT 5 BY x` is nonsense SQL but
    // the detector must still classify total-ly without treating it as GroupOrderList.
    let got = ctx("LIMIT 5 BY ");
    assert!(!matches!(got, CursorContext::GroupOrderList { .. }));
}

#[test]
fn value_trigger_with_non_operator_keyword_before_literal_is_not_value_mode() {
    // `SELECT '` — an open literal whose preceding content token is a non-operator keyword
    // (`SELECT`) yields no value trigger, so it is NOT ColumnValue (exercises the keyword `_ => None`
    // arm in `value_trigger_before`). It falls back to the select-list classification.
    assert!(!matches!(
        ctx("SELECT '"),
        CursorContext::ColumnValue { .. }
    ));
}

#[test]
fn numeric_lhs_is_not_treated_as_a_column() {
    // `WHERE 5 = '` — the token left of `=` is a number, not a column, so `resolve_column` returns
    // None and this is not value mode (guards against pretending a literal is a column).
    assert!(!matches!(
        ctx("WHERE 5 = '"),
        CursorContext::ColumnValue { .. }
    ));
}

// ── §5.6 property: never panics for any byte offset; partial present at cursor ──────────────────

proptest::proptest! {
    /// For ANY query and ANY in-bounds char-boundary cursor, `detect_context` returns without
    /// panicking. (The §5.6 invariant.)
    #[test]
    fn prop_never_panics_for_any_offset(s in ".{0,120}") {
        let toks = tokenize(&s);
        for cursor in 0..=s.len() {
            if s.is_char_boundary(cursor) {
                let _ = detect_context(&s, &toks, cursor);
            }
        }
    }

    /// The reported `partial` is always exactly what the shared lexer extracts at the cursor — the
    /// detector never invents or drops partial text. (Consistency with `partial_at_cursor`.)
    #[test]
    fn prop_partial_matches_lexer(s in "[A-Za-z0-9 ,='()]{0,80}") {
        let toks = tokenize(&s);
        for cursor in 0..=s.len() {
            if !s.is_char_boundary(cursor) {
                continue;
            }
            let expected = crate::sql_lexer::partial_at_cursor(&s, &toks, cursor);
            let got = match detect_context(&s, &toks, cursor) {
                CursorContext::SelectList { partial }
                | CursorContext::FromTable { partial }
                | CursorContext::Predicate { partial }
                | CursorContext::ColumnValue { partial, .. }
                | CursorContext::GroupOrderList { partial }
                | CursorContext::Keyword { partial } => partial,
                // ComparisonOp carries no partial (operator position); skip.
                CursorContext::ComparisonOp { .. } => continue,
            };
            proptest::prop_assert_eq!(got, expected, "partial mismatch at cursor {}", cursor);
        }
    }
}
