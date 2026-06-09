//! Candidate generator — `get_suggestions(query, cursor, &Schema, &OperatorTable, &ValueCache)`
//! (`dev/PLAN.md` §5.4/§5.5/§5.6, `dev/DECISIONS.md` S4/S5).
//!
//! The end of the autocomplete pipeline: tokenize -> detect the [`CursorContext`] -> gather
//! candidates from the right source(s) per the §5.4 mapping table -> fuzzy-rank by the in-progress
//! `partial` -> cap. The single biggest jiq simplification is structural: every source is **plain
//! data** (`&Schema`, the static `&OperatorTable`, a `&ValueCache` *passed as data*), so this is a
//! pure function with **zero engine linkage** — every test hand-builds its inputs and never spins up
//! DuckDB. The engine only ever *fills* the `ValueCache`, out of band through the worker (P3.7).
//!
//! Fuzzy ranking is **in-house and deterministic** rather than the `fuzzy-matcher` crate jiq uses.
//! Two reasons on ciq's own merits: (1) the candidate sets here are tiny and known (a handful of
//! columns, a fixed keyword/function/operator table), so a fuzzy library buys nothing a
//! prefix+subsequence rank doesn't; (2) the determinism rule requires a **total, stable order** for
//! anything user-visible, and an in-house comparator we control is the cleanest way to guarantee
//! that and snapshot-stability without auditing a dependency's tie-break behavior. See
//! [`rank_and_cap`].

use crate::autocomplete::clause_context::{CursorContext, detect_context};
use crate::autocomplete::sql_keywords::{self, FunctionEntry, OperatorTable};
use crate::autocomplete::value_source::ValueCache;
use crate::engine::duckdb_engine::TABLE;
use crate::schema::Schema;
use crate::sql_lexer::tokenize;
use crate::text_match::is_subsequence;

use super::autocomplete_state::{Suggestion, SuggestionType};

/// Maximum number of suggestions returned. Mirrors the popup-size cap jiq applies after ranking; a
/// small fixed number keeps the popup readable and the value list bounded.
pub const MAX_SUGGESTIONS: usize = 50;

/// Generate ranked autocomplete suggestions for `query` with the cursor at byte `cursor`.
///
/// Pure: a function of the query text, the cursor, and three plain-data sources. Total — returns
/// (possibly empty) `Vec<Suggestion>` for any input, never panics (the §5.6 property).
///
/// `schema` supplies column candidates and their [`crate::schema::ColumnType`] hints; `operators`
/// is the static operator table (`sql_keywords::OPERATORS`); `values` is the pre-filled distinct
/// value cache (empty on a miss — the generator simply returns no value candidates, and the worker
/// fills it for the next keystroke in P3.7).
pub fn get_suggestions(
    query: &str,
    cursor: usize,
    schema: &Schema,
    operators: &OperatorTable,
    values: &ValueCache,
) -> Vec<Suggestion> {
    let tokens = tokenize(query);
    let context = detect_context(query, &tokens, cursor);
    gather_for_context(&context, schema, operators, values)
}

/// Branch on the detected context and produce the ranked candidate list for it (the §5.4 table).
fn gather_for_context(
    context: &CursorContext,
    schema: &Schema,
    operators: &OperatorTable,
    values: &ValueCache,
) -> Vec<Suggestion> {
    match context {
        // Columns + `*` + the full function/aggregate table.
        CursorContext::SelectList { partial } => {
            let mut out = column_suggestions(schema);
            out.push(Suggestion::new("*", SuggestionType::Field));
            out.extend(function_suggestions(sql_keywords::FUNCTIONS));
            rank_and_cap(out, partial)
        }
        // The single loaded relation (v1) — one candidate.
        CursorContext::FromTable { partial } => {
            let out = vec![Suggestion::new(TABLE, SuggestionType::Field)];
            rank_and_cap(out, partial)
        }
        // Columns. §5.4 also allows aggregates after HAVING, but the detector collapses WHERE and
        // HAVING into one `Predicate` context, and aggregates are illegal in a bare WHERE (§5.7).
        // Rather than risk leaking aggregates into WHERE, v1 offers columns only here (the always-
        // legal predicate operand); SelectList owns the aggregate table.
        CursorContext::Predicate { partial } => {
            let out = column_suggestions(schema);
            rank_and_cap(out, partial)
        }
        // The static operator table. Empty `partial` (the cursor is at a fresh operator position
        // after `col `) offers the full table in canonical order; a non-empty partial (the user
        // started typing an operator name like `l` for LIKE) filters the table case-insensitively
        // through the same fuzzy ranker the column/keyword branches use.
        CursorContext::ComparisonOp { partial, .. } => {
            rank_and_cap(operator_suggestions(operators), partial)
        }
        // Distinct values of the column, from the cache, fuzzy-filtered by the partial. A cache miss
        // yields an empty list (the worker fills it for the next keystroke — P3.7). The detected
        // column keeps the user's casing; resolve it to the canonical header spelling (DuckDB is
        // case-insensitive for unquoted idents) so the type hint and the cache key match the fetch.
        CursorContext::ColumnValue { col, partial, .. } => {
            let canonical = schema
                .column_ci(col)
                .map(|c| c.name.as_str())
                .unwrap_or(col);
            let field_type = schema.column_type_ci(col).cloned();
            let out = values
                .get(canonical)
                .unwrap_or(&[])
                .iter()
                .map(|v| {
                    Suggestion::new_with_type(v.clone(), SuggestionType::Value, field_type.clone())
                })
                .collect();
            rank_and_cap(out, partial)
        }
        // Columns. The detector keeps the cursor in `GroupOrderList` for the whole `GROUP BY`/
        // `ORDER BY` list (even after a column + `ASC`/`DESC`, §5.4), so this position offers
        // columns; the `ASC`/`DESC` keywords themselves come from the keyword set when the cursor is
        // at a bare keyword position, not here.
        CursorContext::GroupOrderList { partial } => {
            let out = column_suggestions(schema);
            rank_and_cap(out, partial)
        }
        // Position-valid clause keywords.
        CursorContext::Keyword { partial } => {
            let out = keyword_suggestions(sql_keywords::KEYWORDS);
            rank_and_cap(out, partial)
        }
    }
}

/// One `Field` suggestion per schema column, in table order, carrying its `ColumnType` hint.
fn column_suggestions(schema: &Schema) -> Vec<Suggestion> {
    schema
        .columns()
        .iter()
        .map(|c| Suggestion::new_with_type(&c.name, SuggestionType::Field, Some(c.ty.clone())))
        .collect()
}

/// Function/aggregate suggestions from the static table, carrying signature + description.
/// Aggregates get [`SuggestionType::Aggregate`]; scalar functions get [`SuggestionType::Function`].
fn function_suggestions(functions: &[FunctionEntry]) -> Vec<Suggestion> {
    functions
        .iter()
        .map(|f| {
            let kind = if f.is_aggregate {
                SuggestionType::Aggregate
            } else {
                SuggestionType::Function
            };
            Suggestion::new(f.name, kind)
                .with_signature(f.signature)
                .with_description(f.description)
        })
        .collect()
}

/// Operator suggestions from the table, in canonical order (no partial filtering at this position).
fn operator_suggestions(operators: &OperatorTable) -> Vec<Suggestion> {
    operators
        .iter()
        .map(|o| Suggestion::new(o.text, SuggestionType::Operator).with_description(o.label))
        .collect()
}

/// Keyword suggestions from the static clause-keyword table.
fn keyword_suggestions(keywords: &[&str]) -> Vec<Suggestion> {
    keywords
        .iter()
        .map(|kw| Suggestion::new(*kw, SuggestionType::Keyword))
        .collect()
}

/// Fuzzy-rank `candidates` against `partial` and cap to [`MAX_SUGGESTIONS`].
///
/// **Empty partial** — the user is at a fresh position (`WHERE `, `SELECT `); keep every candidate
/// in its incoming (canonical, source) order, capped. ciq lists columns even on an empty partial in
/// clause positions because the column set is small and known (§5.7 "Partial vs. fresh position").
///
/// **Non-empty partial** — case-insensitive. Keep only candidates whose text *fuzzy-matches* the
/// partial, then sort by a **total, deterministic** key so the order is snapshot-stable:
///
/// 1. match tier: exact (0) < prefix (1) < subsequence (2) — better matches first;
/// 2. shorter text first (a tighter match for the same tier);
/// 3. original source index — the canonical tie-break, preserving the table's stable order.
///
/// The sort is `sort_by_key` over that key tuple, which is stable, so equal keys never reorder.
fn rank_and_cap(candidates: Vec<Suggestion>, partial: &str) -> Vec<Suggestion> {
    if partial.is_empty() {
        let mut out = candidates;
        out.truncate(MAX_SUGGESTIONS);
        return out;
    }

    let needle = partial.to_ascii_lowercase();
    // Sort key `(tier, text-len, source-index)` is a total order: better match tier first, then the
    // tighter (shorter) match, then the canonical source order as the final stable tie-break.
    let mut ranked: Vec<((u8, usize, usize), Suggestion)> = candidates
        .into_iter()
        .enumerate()
        .filter_map(|(idx, s)| {
            match_tier(&s.text, &needle).map(|tier| ((tier, s.text.chars().count(), idx), s))
        })
        .collect();

    ranked.sort_by_key(|(key, _)| *key);
    ranked.truncate(MAX_SUGGESTIONS);
    ranked.into_iter().map(|(_, s)| s).collect()
}

/// The match tier of `text` against the already-lowercased `needle`, or `None` if no match.
/// Lower is better: `0` exact, `1` prefix, `2` subsequence. The subsequence check is the shared
/// [`crate::text_match::is_subsequence`] — the same rule the palette filter uses, so the two fuzzy
/// matchers cannot drift.
fn match_tier(text: &str, needle: &str) -> Option<u8> {
    let hay = text.to_ascii_lowercase();
    if hay == needle {
        Some(0)
    } else if hay.starts_with(needle) {
        Some(1)
    } else if is_subsequence(&hay, needle) {
        Some(2)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "candidates_tests.rs"]
mod candidates_tests;
