//! Per-pane Simple-mode autocomplete adapter.
//!
//! The Simple-mode query box is five labeled clause panes (`SELECT` / `WHERE` / `GROUP BY` /
//! `ORDER BY` / `LIMIT`). Each pane holds only its own clause text — `WHERE region = 'EU'`'s pane
//! holds just `region = 'EU'`, no `WHERE` keyword. The autocomplete pipeline (P3.5) is built around
//! a single SQL string with token spans and a cursor offset, so we adapt by **synthesizing the
//! governing clause prefix** and shifting the cursor by its byte length, then handing the result
//! to the existing `clause_context::detect_context` + `candidates::get_suggestions`. This is a
//! deliberate reuse — the clause-context logic is the same regardless of how the bar is partitioned;
//! only the prefix the cursor sees differs.
//!
//! `LIMIT` is numeric-input only (no popup); the helper short-circuits it to an empty list. The
//! `ORDER BY` pane appends `ASC`/`DESC` after a column has been typed, since those are the legal
//! follow-ups in that clause and the synthesized-prefix path classifies them as `GroupOrderList`
//! (which the global detector keeps "column position" for the whole list).
//!
//! Pure: a function of `(pane, pane_text, pane_cursor, &Schema, &OperatorTable, &ValueCache)`.
//! No engine, no terminal, no clock.

use crate::app::query_form::SimplePane;
use crate::autocomplete::autocomplete_state::{Suggestion, SuggestionType};
use crate::autocomplete::candidates::get_suggestions;
use crate::autocomplete::clause_context::{CursorContext, detect_context};
use crate::autocomplete::sql_keywords::OperatorTable;
use crate::autocomplete::value_source::ValueCache;
use crate::schema::Schema;
use crate::sql_lexer::tokenize;

/// The governing clause prefix synthesized in front of a Simple-mode pane's text. Returns `None`
/// for the `LIMIT` pane (numeric only — no popup is offered there).
///
/// `SELECT ` for the SELECT pane, `WHERE ` for WHERE, `GROUP BY ` for GROUP BY, `ORDER BY ` for
/// ORDER BY. The prefix is the literal SQL clause keyword plus a single space — the detector walks
/// backward to that keyword exactly as it would for a Power-mode query containing the same clause.
pub fn synthesized_prefix(pane: SimplePane) -> Option<&'static str> {
    match pane {
        SimplePane::Select => Some("SELECT "),
        SimplePane::Where => Some("WHERE "),
        SimplePane::GroupBy => Some("GROUP BY "),
        SimplePane::OrderBy => Some("ORDER BY "),
        // The LIMIT pane is numeric-only; the popup never opens there.
        SimplePane::Limit => None,
    }
}

/// Build the (synthesized_query, cursor) pair fed to `detect_context` / `get_suggestions` for a
/// Simple-mode pane. `None` when the pane has no completion (the LIMIT pane).
///
/// The cursor in the synthesized query is `prefix.len() + pane_cursor` — pane-text byte offsets
/// stay consistent because the prefix is pure ASCII.
pub fn synthesize(
    pane: SimplePane,
    pane_text: &str,
    pane_cursor: usize,
) -> Option<(String, usize)> {
    let prefix = synthesized_prefix(pane)?;
    let mut q = String::with_capacity(prefix.len() + pane_text.len());
    q.push_str(prefix);
    q.push_str(pane_text);
    let cursor = prefix.len() + pane_cursor.min(pane_text.len());
    Some((q, cursor))
}

/// Per-pane suggestions for the Simple-mode query box. Returns an empty list for the LIMIT pane
/// and for any pane whose synthesized query yields no candidates.
///
/// For the ORDER BY pane, `ASC`/`DESC` keyword suggestions are appended after a column token has
/// been typed in the pane — the canonical follow-ups in an `ORDER BY` list. They sit at the end so
/// the existing column ranking remains the primary surface; an empty pane offers only columns.
pub fn pane_suggestions(
    pane: SimplePane,
    pane_text: &str,
    pane_cursor: usize,
    schema: &Schema,
    operators: &OperatorTable,
    values: &ValueCache,
) -> Vec<Suggestion> {
    let Some((query, cursor)) = synthesize(pane, pane_text, pane_cursor) else {
        return Vec::new();
    };
    let mut out = get_suggestions(&query, cursor, schema, operators, values);

    // ORDER BY pane: once a column token exists in the pane text, also offer ASC / DESC. Detected
    // by re-tokenizing the *pane text alone* (so the synthesized `ORDER BY` prefix doesn't count)
    // and looking for an Ident or QuotedIdent — the only token shapes a column reference takes.
    if matches!(pane, SimplePane::OrderBy) && pane_has_identifier(pane_text) {
        out.extend(asc_desc_keywords());
    }
    out
}

/// Whether the pane text contains at least one identifier-shaped token (the column reference). A
/// pure Ident or QuotedIdent at any depth qualifies — `ORDER BY foo` and `ORDER BY "order"` both
/// trigger the ASC/DESC tail.
fn pane_has_identifier(pane_text: &str) -> bool {
    use crate::sql_lexer::TokenKind;
    tokenize(pane_text)
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Ident | TokenKind::QuotedIdent))
}

/// The `ASC` / `DESC` `Keyword` suggestions appended to the ORDER BY pane after a column.
fn asc_desc_keywords() -> Vec<Suggestion> {
    vec![
        Suggestion::new("ASC", SuggestionType::Keyword),
        Suggestion::new("DESC", SuggestionType::Keyword),
    ]
}

/// The detected [`CursorContext`] for the Simple-mode pane — exposes the same classification the
/// candidate generator runs on, for tests and for the pane-aware value-fetch path.
pub fn pane_context(
    pane: SimplePane,
    pane_text: &str,
    pane_cursor: usize,
) -> Option<CursorContext> {
    let (query, cursor) = synthesize(pane, pane_text, pane_cursor)?;
    let tokens = tokenize(&query);
    Some(detect_context(&query, &tokens, cursor))
}

#[cfg(test)]
#[path = "pane_context_tests.rs"]
mod pane_context_tests;
