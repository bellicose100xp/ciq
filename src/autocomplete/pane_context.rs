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
/// For the ORDER BY pane, `ASC`/`DESC` keyword suggestions are offered when the cursor sits
/// immediately after a typed column token — the canonical follow-ups in an `ORDER BY` list. The
/// keywords are filtered through the same partial the column candidates are ranked against, so
/// typing `D` brings `DESC` to the top instead of leaving it last in a fixed `[ASC, DESC]` tail.
/// After a comma (a fresh list slot), only columns are offered.
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

    // ORDER BY pane: ASC / DESC are legal follow-ups only immediately after a typed column (not
    // at a fresh slot opened by a comma, and not while extending a non-matching partial that
    // ranks no columns). The keywords are filtered through the active partial so they merge into
    // the ranked list correctly — e.g. typing `D` ranks `DESC` ahead of any subsequence-only
    // column match.
    if matches!(pane, SimplePane::OrderBy) {
        let partial = pane_partial_for_order_by(pane_text, pane_cursor);
        if order_by_after_column(pane_text, pane_cursor) {
            for kw in asc_desc_keywords() {
                if keyword_matches_partial(&kw.text, partial) {
                    out.push(kw);
                }
            }
        }
    }
    out
}

/// The text the user is currently typing as the rightmost token in the ORDER BY pane (the
/// "partial"). Empty when the pane ends with whitespace or a comma — the user is at a fresh slot.
fn pane_partial_for_order_by(pane_text: &str, pane_cursor: usize) -> &str {
    let cursor = pane_cursor.min(pane_text.len());
    let head = &pane_text[..cursor];
    let last_break = head
        .rfind(|c: char| c.is_ascii_whitespace() || c == ',')
        .map(|i| i + 1)
        .unwrap_or(0);
    &head[last_break..]
}

/// Whether the cursor sits immediately after a typed column token in the ORDER BY pane (not at a
/// fresh slot opened by a comma, and not at a fully empty pane). The check walks the tokens in
/// the pane text up to the cursor and looks for an `Ident`/`QuotedIdent` whose **last** content
/// token is not a comma — i.e. we're still inside the same list slot.
fn order_by_after_column(pane_text: &str, pane_cursor: usize) -> bool {
    use crate::sql_lexer::TokenKind;
    let cursor = pane_cursor.min(pane_text.len());
    let head = &pane_text[..cursor];
    let tokens = tokenize(head);
    let mut saw_ident = false;
    let mut last_was_comma = false;
    for t in tokens.iter().filter(|t| !t.is_trivia()) {
        match t.kind {
            TokenKind::Ident | TokenKind::QuotedIdent => {
                saw_ident = true;
                last_was_comma = false;
            }
            TokenKind::Punct if t.text(head) == "," => {
                // Comma resets the slot — the user is opening a fresh column position.
                saw_ident = false;
                last_was_comma = true;
            }
            _ => {
                last_was_comma = false;
            }
        }
    }
    saw_ident && !last_was_comma
}

/// The `ASC` / `DESC` `Keyword` suggestions, in canonical order. The caller filters them through
/// the active partial before merging into the ranked list.
fn asc_desc_keywords() -> Vec<Suggestion> {
    vec![
        Suggestion::new("ASC", SuggestionType::Keyword),
        Suggestion::new("DESC", SuggestionType::Keyword),
    ]
}

/// Whether `keyword` is a legitimate match for the user's `partial`. Empty partial -> always; a
/// non-empty partial requires a case-insensitive prefix match (the keyword set is tiny and
/// case-folded, so a prefix rule is the right grain).
fn keyword_matches_partial(keyword: &str, partial: &str) -> bool {
    if partial.is_empty() {
        return true;
    }
    keyword
        .to_ascii_lowercase()
        .starts_with(&partial.to_ascii_lowercase())
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
