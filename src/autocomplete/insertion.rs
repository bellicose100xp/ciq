//! Insert-at-cursor — `insert_suggestion(text, cursor, suggestion) -> (new_text, new_cursor)`
//! (`dev/PLAN.md` §5.1/§5.7, `dev/DECISIONS.md` S5).
//!
//! The reused jiq insert-at-cursor logic (replace the in-progress partial token under the cursor
//! with the chosen suggestion), with the SQL rules ciq adds and jiq's JSON-only bits dropped:
//!
//!  - **SQL identifier quoting on insert (§5.7).** A `Field` whose name collides with a SQL
//!    keyword (`order`, `select`) or contains spaces / characters that aren't a bare identifier is
//!    auto-quoted as `"order"` so the inserted text re-lexes as a `QuotedIdent`, never a keyword.
//!    A `"` inside the name is doubled (`we"ird` -> `"we""ird"`), the SQL-standard escape — the
//!    same shared [`sql_ident::quote_ident`](crate::sql_ident::quote_ident) every emitter uses,
//!    kept consistent here.
//!  - **`Value` suggestions are quoted as string literals** for text/temporal columns (`'active'`),
//!    so completing `WHERE status = ` then `a` inserts `'active'`. Numeric values are inserted bare.
//!  - **jiq's `needs_leading_dot` is dropped** — there is no SQL analog (jq path completion needed
//!    a `.`; SQL columns are bare identifiers).
//!
//! Pure and total: a function of `(text, cursor_byte, suggestion)` returning the new text and the
//! new byte cursor. It never panics for any cursor in `0..=text.len()` and round-trips UTF-8 — the
//! cursor always lands on a char boundary in the new string (the §5.6 property tested in
//! `insertion_tests`). It works in **byte** offsets (matching the lexer / detector / candidate
//! generator, which are all byte-indexed); the App converts its character cursor at the seam.

use crate::sql_ident::{quote_ident_if_needed, render_typed_value};
use crate::sql_lexer::{TokenKind, tokenize};

use super::autocomplete_state::{Suggestion, SuggestionType};

/// Replace the partial token at `cursor` (a byte offset into `text`) with `suggestion`, returning
/// the new text and the new byte cursor (positioned just after the inserted text).
///
/// The "partial token" is the identifier/keyword/quoted-ident/open-string token the cursor is
/// extending; its span is replaced wholesale. When the cursor is not on such a token (e.g. just
/// after a space, or on punctuation), the suggestion is inserted at the cursor with nothing
/// removed.
///
/// The inserted text is the suggestion's [`render_insert_text`] — already quoted per the SQL rules
/// above. For a value literal opened by a `'` already in the buffer, the opening quote is part of
/// the replaced span (see [`partial_span`]), so the emitted `'value'` does not double the quote.
pub fn insert_suggestion(text: &str, cursor: usize, suggestion: &Suggestion) -> (String, usize) {
    // Snap the cursor onto a UTF-8 boundary at or before it, so a caller passing a mid-char byte
    // offset (the §5.6 property covers arbitrary offsets) can never make the slicing panic.
    let cursor = floor_char_boundary(text, cursor.min(text.len()));
    let insert = render_insert_text(suggestion);
    let (start, end) = partial_span(text, cursor);

    let mut out = String::with_capacity(text.len() - (end - start) + insert.len());
    out.push_str(&text[..start]);
    out.push_str(&insert);
    out.push_str(&text[end..]);
    let new_cursor = start + insert.len();
    (out, new_cursor)
}

/// The exact text a suggestion inserts, with SQL quoting applied per kind (§5.7):
///  - `Field` colliding with a keyword or not a bare identifier -> `"quoted"`;
///  - `Value` for a text/temporal column -> `'string literal'` (numeric values stay bare);
///  - everything else (keywords, operators, functions, plain columns) -> verbatim.
pub fn render_insert_text(s: &Suggestion) -> String {
    match s.suggestion_type {
        // The all-columns wildcard `*` is offered as a `Field` in the SELECT-list context and
        // inserts bare (it is the wildcard token, not an identifier). Any other field name is
        // identifier-quoted as needed — so a real column whose name happens to contain `*` would
        // still be quoted by `quote_ident_if_needed`; only the literal lone `*` wildcard is bare.
        SuggestionType::Field if s.text == "*" => "*".to_string(),
        SuggestionType::Field => quote_ident_if_needed(&s.text),
        // A value suggestion is rendered bare for a numeric/bool column with a bare-safe value and
        // single-quoted otherwise — via the one shared [`render_typed_value`], so insertion and the
        // palette emitter apply the exact same bare-vs-quote rule (no drift).
        SuggestionType::Value => render_typed_value(&s.text, s.field_type.as_ref()),
        _ => s.text.clone(),
    }
}

/// The largest char boundary of `text` that is `<= byte`. `str::floor_char_boundary` is unstable,
/// so this open-codes it: walk back from `byte` until the index is a boundary.
fn floor_char_boundary(text: &str, byte: usize) -> usize {
    let mut b = byte.min(text.len());
    while b > 0 && !text.is_char_boundary(b) {
        b -= 1;
    }
    b
}

/// The byte span `[start, end)` of the partial token the cursor at `cursor` is extending — the
/// text [`insert_suggestion`] replaces. Returns `(cursor, cursor)` (an empty span = pure insert)
/// when the cursor is not on a replaceable partial.
///
/// Replaceable partials: an `Ident`/`Keyword`/`QuotedIdent`/`Number` the cursor is inside or at the
/// end of, or an **open** string literal the cursor is inside (its opening `'` is included so a
/// value suggestion `'active'` replaces `'a` cleanly, not appends after it). The span never extends
/// past the cursor — text to the right of the cursor is preserved (mid-query edits).
fn partial_span(text: &str, cursor: usize) -> (usize, usize) {
    let tokens = tokenize(text);
    for t in &tokens {
        if t.is_trivia() {
            continue;
        }
        let inside = t.start < cursor && cursor <= t.end;
        let at_start = t.start == cursor;
        match t.kind {
            TokenKind::Ident | TokenKind::Keyword | TokenKind::QuotedIdent | TokenKind::Number => {
                if inside {
                    return (t.start, cursor);
                }
                if at_start {
                    return (cursor, cursor);
                }
            }
            TokenKind::StringLit { closed: false } if inside => {
                return (t.start, cursor);
            }
            _ => {}
        }
    }
    (cursor, cursor)
}

#[cfg(test)]
#[path = "insertion_tests.rs"]
mod insertion_tests;
