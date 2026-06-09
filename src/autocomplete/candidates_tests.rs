//! Tests for the candidate generator (`dev/PLAN.md` §5.4/§5.5/§5.6/§5.7).
//!
//! Table-driven golden cases over a **fixed in-memory `Schema`**, the static `sql_keywords::OPERATORS`
//! table, and a **hand-seeded `ValueCache`** — no engine. Covers every §5.4 mapping row and the
//! §5.7 cases that reach the candidate generator (aggregates only in SelectList; LIKE -> values not
//! operators; quoted-ident column match; IN-list value mode). A `proptest` asserts the §5.6
//! never-panic property over arbitrary `(query, cursor)`.

use super::*;
use crate::autocomplete::sql_keywords::OPERATORS;
use crate::autocomplete::value_source::ValueCache;
use crate::engine::duckdb_engine::TABLE;
use crate::schema::{ColumnMeta, ColumnType, Schema};

/// The fixed test schema: a column per `ColumnType` family, including one whose name collides with a
/// SQL keyword (`order`) and one with an underscore (`created_at`) so type hints and quoted-ident
/// matching are both exercised.
fn schema() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("status", ColumnType::Text),
        ColumnMeta::new("amount", ColumnType::Float),
        ColumnMeta::new("created_at", ColumnType::Date),
        ColumnMeta::new("order", ColumnType::Int),
    ])
}

/// A `ValueCache` seeded with distinct values for `status` (frequency-ordered, as the engine would
/// return them) and `order` so quoted-ident value mode is testable.
fn seeded_cache() -> ValueCache {
    let mut c = ValueCache::new();
    c.insert(
        "status",
        vec!["active".into(), "archived".into(), "pending".into()],
    );
    c.insert("order", vec!["1".into(), "2".into()]);
    c
}

/// Run the generator with the cursor at end-of-input (the common live-typing position).
fn suggest(query: &str) -> Vec<Suggestion> {
    let schema = schema();
    let cache = seeded_cache();
    get_suggestions(query, query.len(), &schema, OPERATORS, &cache)
}

/// Run the generator with the cursor at a specific byte offset (interior-cursor cases).
fn suggest_at(query: &str, cursor: usize) -> Vec<Suggestion> {
    let schema = schema();
    let cache = seeded_cache();
    get_suggestions(query, cursor, &schema, OPERATORS, &cache)
}

/// The `text` field of every suggestion, in returned order.
fn texts(s: &[Suggestion]) -> Vec<&str> {
    s.iter().map(|x| x.text.as_str()).collect()
}

/// Whether any suggestion has the given kind.
fn has_kind(s: &[Suggestion], kind: SuggestionType) -> bool {
    s.iter().any(|x| x.suggestion_type == kind)
}

// ── §5.4 row: SelectList ────────────────────────────────────────────────────────────────────────

#[test]
fn select_list_offers_columns_star_and_functions() {
    let s = suggest("SELECT ");
    let t = texts(&s);
    // Columns appear first (canonical schema order), then `*`, then the function table.
    assert!(t.contains(&"id"));
    assert!(t.contains(&"status"));
    assert!(t.contains(&"*"));
    assert!(t.contains(&"COUNT"));
    assert!(t.contains(&"lower"));
    // Aggregates and scalar functions are distinguished by kind.
    assert!(has_kind(&s, SuggestionType::Field));
    assert!(has_kind(&s, SuggestionType::Aggregate));
    assert!(has_kind(&s, SuggestionType::Function));
}

#[test]
fn select_list_column_carries_type_hint() {
    let s = suggest("SELECT ");
    let created = s.iter().find(|x| x.text == "created_at").unwrap();
    assert_eq!(created.field_type, Some(ColumnType::Date));
    let amount = s.iter().find(|x| x.text == "amount").unwrap();
    assert_eq!(amount.field_type, Some(ColumnType::Float));
}

#[test]
fn select_list_function_carries_signature_and_description() {
    let s = suggest("SELECT ");
    let count = s.iter().find(|x| x.text == "COUNT").unwrap();
    assert_eq!(count.suggestion_type, SuggestionType::Aggregate);
    assert_eq!(count.signature.as_deref(), Some("COUNT(expr)"));
    assert_eq!(count.description.as_deref(), Some("count of non-null rows"));
}

#[test]
fn select_list_filters_by_partial() {
    // `SELECT cou` -> COUNT ranks (prefix). `coalesce` is a subsequence match (c..o..a..l → no:
    // needs c,o,u in order; coalesce has no 'u'), so it is filtered out.
    let s = suggest("SELECT cou");
    let t = texts(&s);
    assert!(t.contains(&"COUNT"));
    assert!(!t.contains(&"coalesce"));
}

#[test]
fn select_list_inside_aggregate_paren_offers_columns() {
    // `SELECT COUNT(` -> still SelectList: columns + `*` + functions.
    let s = suggest("SELECT COUNT(");
    let t = texts(&s);
    assert!(t.contains(&"id"));
    assert!(t.contains(&"*"));
}

// ── §5.4 row: FromTable ─────────────────────────────────────────────────────────────────────────

#[test]
fn from_table_offers_the_single_relation() {
    let s = suggest("SELECT * FROM ");
    assert_eq!(texts(&s), vec![TABLE]);
}

#[test]
fn from_table_filters_by_partial() {
    // Partial `t` matches the table named `t`; partial `z` matches nothing.
    assert_eq!(texts(&suggest("SELECT * FROM t")), vec![TABLE]);
    assert!(suggest("SELECT * FROM z").is_empty());
}

// ── §5.4 row: Predicate ─────────────────────────────────────────────────────────────────────────

#[test]
fn predicate_offers_columns_only_no_aggregates() {
    // §5.7: aggregates are illegal in a bare WHERE — v1 offers columns only.
    let s = suggest("SELECT * FROM t WHERE ");
    let t = texts(&s);
    assert!(t.contains(&"status"));
    assert!(t.contains(&"amount"));
    assert!(!has_kind(&s, SuggestionType::Aggregate));
    assert!(!t.contains(&"COUNT"));
}

#[test]
fn predicate_after_and_offers_columns() {
    let s = suggest("SELECT * FROM t WHERE id > 0 AND st");
    let t = texts(&s);
    assert!(t.contains(&"status"));
    // `st` does not prefix-match `id`/`amount` and is not a subsequence of them.
    assert!(!t.contains(&"id"));
}

// ── §5.4 row: ComparisonOp ──────────────────────────────────────────────────────────────────────

#[test]
fn comparison_op_offers_the_operator_table() {
    // `WHERE status ` -> the full operator table, in canonical order.
    let s = suggest("SELECT * FROM t WHERE status ");
    let t = texts(&s);
    assert_eq!(t.first(), Some(&"="));
    assert!(t.contains(&"LIKE"));
    assert!(t.contains(&"IS NOT NULL"));
    assert!(
        s.iter()
            .all(|x| x.suggestion_type == SuggestionType::Operator)
    );
}

#[test]
fn comparison_op_filters_operators_by_partial_l_keeps_like() {
    // The user's exact bug repro: `WHERE col l|` keeps the operator popup OPEN with operators
    // matching `l` (only `LIKE` from the table, since other operators don't start with `l`/`L`).
    let s = suggest("SELECT * FROM t WHERE status l");
    let t = texts(&s);
    assert!(
        t.contains(&"LIKE"),
        "popup must keep LIKE when partial is `l`: {t:?}"
    );
    assert!(
        s.iter()
            .all(|x| x.suggestion_type == SuggestionType::Operator),
        "filtered list must still be operators only: {t:?}"
    );
}

#[test]
fn comparison_op_filters_operators_by_partial_b_keeps_between() {
    // Same shape with `b` -> `BETWEEN`.
    let s = suggest("SELECT * FROM t WHERE status b");
    let t = texts(&s);
    assert!(t.contains(&"BETWEEN"), "expected BETWEEN in {t:?}");
    assert!(
        s.iter()
            .all(|x| x.suggestion_type == SuggestionType::Operator)
    );
}

#[test]
fn comparison_op_filters_operators_by_partial_i_keeps_in_and_is_variants() {
    // `i` matches `IN`, `IS NULL`, `IS NOT NULL` — all three should survive the filter.
    let s = suggest("SELECT * FROM t WHERE status i");
    let t = texts(&s);
    assert!(t.contains(&"IN"), "expected IN in {t:?}");
    assert!(t.contains(&"IS NULL"), "expected IS NULL in {t:?}");
    assert!(t.contains(&"IS NOT NULL"), "expected IS NOT NULL in {t:?}");
}

#[test]
fn comparison_op_filters_case_insensitively_uppercase_partial() {
    // The fuzzy ranker is case-insensitive (`L` matches `LIKE` just like `l` does).
    let s = suggest("SELECT * FROM t WHERE status L");
    assert!(texts(&s).contains(&"LIKE"));
}

// ── §5.4 row: ColumnValue ───────────────────────────────────────────────────────────────────────

#[test]
fn column_value_offers_distinct_values_from_cache() {
    // `WHERE status = '` -> distinct values of `status`, typed by the column's type.
    let s = suggest("SELECT * FROM t WHERE status = '");
    let t = texts(&s);
    assert_eq!(t, vec!["active", "archived", "pending"]);
    assert!(s.iter().all(|x| x.suggestion_type == SuggestionType::Value));
    assert!(s.iter().all(|x| x.field_type == Some(ColumnType::Text)));
}

#[test]
fn column_value_filters_by_partial_inside_literal() {
    // `WHERE status = 'a` -> only values matching `a` (prefix), frequency order preserved.
    let s = suggest("SELECT * FROM t WHERE status = 'a");
    assert_eq!(texts(&s), vec!["active", "archived"]);
}

#[test]
fn column_value_cache_miss_yields_empty() {
    // `amount` has no seeded values -> empty list (worker fills it next keystroke, P3.7).
    let s = suggest("SELECT * FROM t WHERE amount = '");
    assert!(s.is_empty());
}

#[test]
fn like_predicate_is_value_mode_not_operators() {
    // §5.7: LIKE -> offer distinct values, NOT operator suggestions (the inverse of jiq).
    let s = suggest("SELECT * FROM t WHERE status LIKE '");
    let t = texts(&s);
    assert_eq!(t, vec!["active", "archived", "pending"]);
    assert!(!has_kind(&s, SuggestionType::Operator));
}

#[test]
fn in_list_is_value_mode_for_the_column() {
    // §5.7: `WHERE status IN ('a', '` -> still ColumnValue for `status`.
    let s = suggest("SELECT * FROM t WHERE status IN ('active', '");
    let t = texts(&s);
    assert_eq!(t, vec!["active", "archived", "pending"]);
    assert!(s.iter().all(|x| x.suggestion_type == SuggestionType::Value));
}

#[test]
fn quoted_ident_column_value_mode() {
    // §5.7: a column named after a keyword is quoted; value mode resolves it to the bare name.
    let s = suggest("SELECT * FROM t WHERE \"order\" = '");
    assert_eq!(texts(&s), vec!["1", "2"]);
}

#[test]
fn value_mode_resolves_column_case_insensitively() {
    // DuckDB resolves unquoted identifiers case-insensitively, so `STATUS` against a `status`
    // header is valid SQL — and its seeded distinct values must still surface, typed by `status`.
    let s = suggest("SELECT * FROM t WHERE STATUS = '");
    assert_eq!(texts(&s), vec!["active", "archived", "pending"]);
    assert!(s.iter().all(|x| x.field_type == Some(ColumnType::Text)));
    // A mixed-case reference resolves the same way.
    let s2 = suggest("SELECT * FROM t WHERE Status = 'a");
    assert_eq!(texts(&s2), vec!["active", "archived"]);
}

// ── completed predicate / IS -> clause keywords, not columns ────────────────────────────────────

#[test]
fn completed_predicate_offers_clause_keywords_not_columns() {
    // After a complete `col = value`, the popup must offer connector/clause keywords, never a list
    // of schema columns (which can't legally follow a finished predicate).
    let s = suggest("SELECT * FROM t WHERE status = 'active' ");
    let t = texts(&s);
    assert!(t.contains(&"AND"));
    assert!(t.contains(&"ORDER BY"));
    assert!(
        !t.contains(&"amount"),
        "no columns after a complete predicate: {t:?}"
    );
    assert!(
        s.iter()
            .all(|x| x.suggestion_type == SuggestionType::Keyword)
    );
}

#[test]
fn after_is_offers_clause_keywords_not_columns() {
    // After a typed `IS`, only NULL/NOT NULL are legal — the position offers keywords, not columns.
    let s = suggest("SELECT * FROM t WHERE status IS ");
    let t = texts(&s);
    assert!(!t.contains(&"amount"), "no columns after IS: {t:?}");
    assert!(
        s.iter()
            .all(|x| x.suggestion_type == SuggestionType::Keyword)
    );
}

// ── §5.4 row: GroupOrderList ────────────────────────────────────────────────────────────────────

#[test]
fn group_by_offers_columns() {
    let s = suggest("SELECT status FROM t GROUP BY ");
    let t = texts(&s);
    assert!(t.contains(&"status"));
    assert!(has_kind(&s, SuggestionType::Field));
    assert!(!has_kind(&s, SuggestionType::Aggregate));
}

#[test]
fn order_by_offers_columns() {
    let s = suggest("SELECT * FROM t ORDER BY ");
    assert!(texts(&s).contains(&"created_at"));
}

#[test]
fn order_by_after_column_stays_column_list() {
    // The detector keeps `ORDER BY id ` in `GroupOrderList` (it does not flip to a keyword position
    // after a column), so this offers columns — the cursor is a fresh sort-key position.
    let s = suggest("SELECT * FROM t ORDER BY id ");
    let t = texts(&s);
    assert!(t.contains(&"status"));
    assert!(s.iter().all(|x| x.suggestion_type == SuggestionType::Field));
}

// ── §5.4 row: Keyword ───────────────────────────────────────────────────────────────────────────

#[test]
fn bare_position_offers_clause_keywords() {
    let s = suggest("");
    let t = texts(&s);
    assert!(t.contains(&"SELECT"));
    assert!(
        s.iter()
            .all(|x| x.suggestion_type == SuggestionType::Keyword)
    );
}

#[test]
fn keyword_filters_by_partial() {
    // Start-of-query partial `sel` is a bare keyword position -> SELECT (prefix). FROM/WHERE don't
    // match `sel` and are excluded.
    let s = suggest("sel");
    let t = texts(&s);
    assert!(t.contains(&"SELECT"));
    assert!(!t.contains(&"FROM"));
    assert!(
        s.iter()
            .all(|x| x.suggestion_type == SuggestionType::Keyword)
    );
}

// ── ranking determinism ─────────────────────────────────────────────────────────────────────────

#[test]
fn ranking_orders_prefix_matches_shorter_first() {
    // Partial `co`: `COUNT` (5) and `coalesce` (8) both prefix-match; shorter text ranks first.
    // `lower` ('l' first) is not a match and is excluded.
    let s = suggest("SELECT co");
    let t = texts(&s);
    let count = t.iter().position(|x| *x == "COUNT").unwrap();
    let coalesce = t.iter().position(|x| *x == "coalesce").unwrap();
    assert!(count < coalesce, "shorter prefix match ranks first: {t:?}");
    assert!(!t.contains(&"lower"));
}

#[test]
fn ranking_prefers_prefix_over_subsequence() {
    // Partial `am`: `amount` prefix-matches (tier 1); `created_at` is a subsequence (a..m, tier 2).
    // The prefix match must rank ahead of the subsequence match.
    let s = suggest("SELECT * FROM t WHERE am");
    let t = texts(&s);
    let amount = t.iter().position(|x| *x == "amount").unwrap();
    let created = t.iter().position(|x| *x == "created_at");
    if let Some(created) = created {
        assert!(
            amount < created,
            "prefix match ranks ahead of subsequence: {t:?}"
        );
    }
}

#[test]
fn ranking_prefers_exact_over_prefix() {
    // Partial `count` in SelectList: `COUNT` is an exact match (tier 0). No other candidate is a
    // tighter match, so it leads its tier.
    let s = suggest("SELECT count");
    assert_eq!(texts(&s).first(), Some(&"COUNT"));
}

#[test]
fn ranking_is_deterministic_across_runs() {
    // The generator is pure; identical inputs produce byte-identical output order.
    let a = suggest("SELECT s");
    let b = suggest("SELECT s");
    assert_eq!(a, b);
}

#[test]
fn empty_partial_keeps_canonical_source_order() {
    // `SELECT ` -> columns first in schema order, then `*`.
    let s = suggest("SELECT ");
    let t = texts(&s);
    assert_eq!(&t[..5], &["id", "status", "amount", "created_at", "order"]);
    assert_eq!(t[5], "*");
}

// ── interior cursor ─────────────────────────────────────────────────────────────────────────────

#[test]
fn classifies_from_token_under_interior_cursor() {
    // §5.7 mid-query: `SELECT a, |b FROM t` — cursor after the comma+space classifies as SelectList.
    let q = "SELECT id, status FROM t";
    let cursor = "SELECT id, ".len();
    let s = suggest_at(q, cursor);
    let t = texts(&s);
    assert!(t.contains(&"status"));
    assert!(t.contains(&"*"));
}

// ── §5.6 never-panic property ───────────────────────────────────────────────────────────────────

use proptest::prelude::*;

proptest! {
    #[test]
    fn never_panics_for_any_query_and_cursor(q in ".{0,80}", frac in 0usize..=100) {
        let schema = schema();
        let cache = seeded_cache();
        // Clamp the cursor to a char boundary in 0..=len so we exercise interior + boundary offsets
        // without ever slicing mid-codepoint (the lexer is total; the generator must be too).
        let len = q.len();
        let mut cursor = (len * frac) / 100;
        while cursor < len && !q.is_char_boundary(cursor) {
            cursor += 1;
        }
        let _ = get_suggestions(&q, cursor, &schema, OPERATORS, &cache);
    }
}
