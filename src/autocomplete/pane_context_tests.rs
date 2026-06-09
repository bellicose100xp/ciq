//! Tests for the per-pane Simple-mode autocomplete adapter.
//!
//! Table-driven over a fixed in-memory `Schema` + `ValueCache` — the `OPERATORS` table is the static
//! production one. Asserts the §5.4 mapping holds when the prefix is synthesized from the focused
//! pane: SELECT pane offers columns + functions; WHERE pane offers columns at start, operators
//! after a column, values after `op '`; GROUP BY / ORDER BY offer columns; ORDER BY also offers
//! ASC/DESC after a column; LIMIT offers nothing.

use super::*;
use crate::app::query_form::SimplePane;
use crate::autocomplete::autocomplete_state::SuggestionType;
use crate::autocomplete::sql_keywords::OPERATORS;
use crate::autocomplete::value_source::ValueCache;
use crate::schema::{ColumnMeta, ColumnType, Schema};

fn schema() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("status", ColumnType::Text),
        ColumnMeta::new("amount", ColumnType::Float),
        ColumnMeta::new("region", ColumnType::Text),
    ])
}

fn cache() -> ValueCache {
    let mut c = ValueCache::new();
    c.insert("region", vec!["EU".into(), "NA".into(), "APAC".into()]);
    c.insert("status", vec!["active".into(), "pending".into()]);
    c
}

fn texts(s: &[Suggestion]) -> Vec<&str> {
    s.iter().map(|x| x.text.as_str()).collect()
}

fn pane_at(pane: SimplePane, text: &str) -> Vec<Suggestion> {
    pane_suggestions(pane, text, text.len(), &schema(), OPERATORS, &cache())
}

// ── SELECT pane ────────────────────────────────────────────────────────────────────────────────

#[test]
fn select_pane_empty_offers_columns_star_and_functions() {
    let s = pane_at(SimplePane::Select, "");
    let t = texts(&s);
    assert!(t.contains(&"id"));
    assert!(t.contains(&"status"));
    assert!(t.contains(&"*"));
    assert!(t.contains(&"COUNT"));
    assert!(t.contains(&"lower"));
}

#[test]
fn select_pane_partial_filters_to_matching_candidates() {
    let s = pane_at(SimplePane::Select, "stat");
    let t = texts(&s);
    assert!(t.contains(&"status"));
    assert!(!t.contains(&"id"), "no `id` for partial `stat`: {t:?}");
}

// ── WHERE pane ─────────────────────────────────────────────────────────────────────────────────

#[test]
fn where_pane_empty_offers_columns_only() {
    let s = pane_at(SimplePane::Where, "");
    let t = texts(&s);
    assert!(t.contains(&"region"));
    assert!(t.contains(&"status"));
    assert!(!t.contains(&"COUNT"), "no aggregates in WHERE: {t:?}");
}

#[test]
fn where_pane_after_column_offers_operators() {
    let s = pane_at(SimplePane::Where, "region ");
    let t = texts(&s);
    assert!(t.contains(&"="));
    assert!(t.contains(&"LIKE"));
    assert!(t.iter().all(|name| !["region", "status"].contains(name)));
}

#[test]
fn where_pane_after_op_quote_offers_values_for_column() {
    let s = pane_at(SimplePane::Where, "region = '");
    let t = texts(&s);
    assert_eq!(t, vec!["EU", "NA", "APAC"]);
    assert!(s.iter().all(|x| x.suggestion_type == SuggestionType::Value));
}

#[test]
fn where_pane_after_and_offers_columns() {
    let s = pane_at(SimplePane::Where, "id = 1 AND ");
    let t = texts(&s);
    assert!(t.contains(&"region"));
    assert!(t.contains(&"status"));
}

// ── GROUP BY pane ──────────────────────────────────────────────────────────────────────────────

#[test]
fn group_by_pane_offers_columns() {
    let s = pane_at(SimplePane::GroupBy, "");
    let t = texts(&s);
    assert!(t.contains(&"region"));
    assert!(t.contains(&"status"));
    // no operators, no aggregates
    assert!(!t.contains(&"="));
    assert!(!t.contains(&"COUNT"));
}

#[test]
fn group_by_pane_partial_filters_columns() {
    let s = pane_at(SimplePane::GroupBy, "reg");
    let t = texts(&s);
    assert!(t.contains(&"region"));
}

// ── ORDER BY pane ──────────────────────────────────────────────────────────────────────────────

#[test]
fn order_by_pane_empty_offers_columns_no_asc_desc() {
    // Empty pane — no column typed yet; ASC/DESC are not yet legal follow-ups.
    let s = pane_at(SimplePane::OrderBy, "");
    let t = texts(&s);
    assert!(t.contains(&"region"));
    assert!(!t.contains(&"ASC"), "no ASC before a column: {t:?}");
    assert!(!t.contains(&"DESC"), "no DESC before a column: {t:?}");
}

#[test]
fn order_by_pane_after_column_offers_columns_and_asc_desc() {
    // After a column, ASC/DESC join the candidate list.
    let s = pane_at(SimplePane::OrderBy, "region ");
    let t = texts(&s);
    assert!(t.contains(&"ASC"));
    assert!(t.contains(&"DESC"));
}

#[test]
fn order_by_pane_partial_d_filters_to_desc_only_not_asc() {
    // Regression: pre-fix `region D` left ASC selected (index 0) because the [ASC, DESC] tail
    // bypassed the partial filter. Now `D` matches `DESC` and excludes `ASC`.
    let s = pane_at(SimplePane::OrderBy, "region D");
    let t = texts(&s);
    assert!(
        t.contains(&"DESC"),
        "DESC must rank for partial 'D', got {t:?}"
    );
    assert!(
        !t.contains(&"ASC"),
        "ASC must not rank for partial 'D', got {t:?}"
    );
}

#[test]
fn order_by_pane_non_matching_partial_drops_asc_desc() {
    // Regression: pre-fix `region X` still appended [ASC, DESC] even though `X` matches neither
    // — the popup pre-selected ASC and Tab silently inserted the wrong keyword. Now the keywords
    // are filtered through the partial, so a non-matching partial yields no ASC/DESC.
    let s = pane_at(SimplePane::OrderBy, "region X");
    let t = texts(&s);
    assert!(!t.contains(&"ASC"), "no ASC for partial 'X': {t:?}");
    assert!(!t.contains(&"DESC"), "no DESC for partial 'X': {t:?}");
}

#[test]
fn order_by_pane_after_comma_does_not_offer_asc_desc() {
    // Regression: pre-fix `region, ` (fresh list slot) still offered ASC/DESC because
    // `pane_has_identifier` saw the earlier `region` ident. Now ASC/DESC only fire while the
    // cursor sits in the same list slot as the typed column.
    let s = pane_at(SimplePane::OrderBy, "region, ");
    let t = texts(&s);
    assert!(!t.contains(&"ASC"), "no ASC after comma: {t:?}");
    assert!(!t.contains(&"DESC"), "no DESC after comma: {t:?}");
}

#[test]
fn order_by_pane_after_comma_partial_does_not_offer_asc_desc() {
    let s = pane_at(SimplePane::OrderBy, "region, st");
    let t = texts(&s);
    assert!(!t.contains(&"ASC"), "no ASC after comma+partial: {t:?}");
    assert!(!t.contains(&"DESC"), "no DESC after comma+partial: {t:?}");
}

// ── LIMIT pane ─────────────────────────────────────────────────────────────────────────────────

#[test]
fn limit_pane_offers_no_suggestions() {
    // LIMIT is numeric-only — the popup must never open in that pane.
    assert!(pane_at(SimplePane::Limit, "").is_empty());
    assert!(pane_at(SimplePane::Limit, "100").is_empty());
}
