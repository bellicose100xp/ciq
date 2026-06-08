//! Clause-context detector — `detect_context(&[Token], cursor) -> CursorContext` (`dev/PLAN.md`
//! §5.3/§5.4/§5.7, `dev/DECISIONS.md` S5).
//!
//! Replaces jiq's `analyze_context` (which walks jq path syntax). The job: given the shared-lexer
//! token stream and a byte cursor, decide *what kind of thing the user is about to type* so the
//! candidate generator (P3.5) knows whether to offer columns, the table, operators, distinct
//! values, or keywords.
//!
//! **Why this is far simpler than jiq.** jiq must reconcile a cache that may be ahead of or behind
//! the cursor against a JSON shape it inferred by sampling (`is_cursor_at_logical_end` /
//! `is_in_non_executing_context`). ciq's schema is *declared*, so there is no cache-vs-cursor
//! branching: find the token at/just-before the cursor, then walk **backward** over preceding
//! non-trivia tokens to the nearest governing clause keyword. That backward scan plus a paren-depth
//! check is the entire algorithm. We deliberately do **not** port jiq's branching (the brief's
//! "do NOT port `is_cursor_at_logical_end`").
//!
//! Total and pure: returns a valid `CursorContext` for *any* token slice and *any* cursor (the
//! §5.6 property), never panics, no I/O.

use crate::sql_lexer::{Token, TokenKind, partial_at_cursor, token_at_cursor};

/// What the cursor is positioned to complete. The canonical S5 enum (`dev/DECISIONS.md` S5,
/// matching `dev/PLAN.md` §5.3/§5.4). One variant per row of the §5.4 mapping table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorContext {
    /// After `SELECT`, or after a comma in the select list. Expect columns / `*` / functions.
    SelectList { partial: String },
    /// After `FROM` / `JOIN`. Expect the table relation name.
    FromTable { partial: String },
    /// After `WHERE` / `AND` / `OR` / `HAVING` / `ON`. Expect a column (a predicate LHS).
    Predicate { partial: String },
    /// After a column in a predicate (`WHERE col `). Expect a comparison operator. `lhs_col` is
    /// informational (titles the popup); the operator candidates come from the operator table.
    ComparisonOp { lhs_col: Option<String> },
    /// Inside a value literal after `col <op>` (`WHERE col = '`, `col IN ('`, `col LIKE '`).
    /// Expect the distinct *values* of `col`. `kind` records which operator triggered it.
    ColumnValue {
        col: String,
        kind: TriggerKind,
        partial: String,
    },
    /// After `GROUP BY` / `ORDER BY`, or a comma therein. Expect a column.
    GroupOrderList { partial: String },
    /// A bare position where a SQL clause keyword is expected (start of query, or after a complete
    /// clause). Expect keywords.
    Keyword { partial: String },
}

/// Which comparison triggered a [`CursorContext::ColumnValue`]. ciq-native (SQL), **not** jiq's
/// jq-predicate `TriggerKind` (`Contains`/`StartsWith`/… have no SQL analog): we keep only the
/// distinctions that change value-completion behavior in SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerKind {
    /// `col = '…'` — equality. The most common case.
    Eq,
    /// `col != '…'` / `col <> '…'` — inequality.
    Neq,
    /// `col < '…'` / `<=` / `>` / `>=` — an ordered comparison against a literal.
    Cmp,
    /// `col LIKE '…'` — pattern match. ciq treats this as **value mode** (offer distinct values),
    /// a deliberate, documented dialect choice — the inverse of jiq, which *suppresses* value
    /// autocomplete for its regex functions (`test`/`match`). See §5.7.
    Like,
    /// `col IN ('…'` — membership list. Still value mode for `col`.
    In,
}

/// Detect the cursor context over the lexer token stream. `cursor` is a byte offset into the
/// original source; `tokens` is `sql_lexer::tokenize(src)`.
///
/// The cursor's `partial` (the in-progress token text) comes from the lexer's
/// [`partial_at_cursor`]; this function only classifies *intent* by walking backward from the
/// cursor token to the governing clause keyword.
pub fn detect_context(src: &str, tokens: &[Token], cursor: usize) -> CursorContext {
    let partial = partial_at_cursor(src, tokens, cursor);

    // If the cursor sits inside an (open) string literal, the user is typing a value. Resolve the
    // column + trigger from what precedes the literal, regardless of clause. This must come first:
    // a quote opens value mode even mid-`SELECT`-list nonsense.
    if let Some(lit_idx) = open_string_literal_at(tokens, cursor)
        && let Some((col, kind)) = value_trigger_before(src, tokens, lit_idx)
    {
        return CursorContext::ColumnValue { col, kind, partial };
    }

    // Index of the content token the cursor is on/extending (skips trivia). `None` => the cursor is
    // on whitespace; we still classify from the tokens to its left.
    let cursor_tok = token_at_cursor(tokens, cursor);

    // The index just before the cursor's own (in-progress) token — the start of the backward walk.
    // When the cursor is on whitespace, that's the last content token strictly before `cursor`.
    let scan_from = match cursor_tok {
        Some(idx) => idx,
        None => match last_content_before(tokens, cursor) {
            Some(idx) => idx + 1, // walk strictly before this token
            None => return CursorContext::Keyword { partial },
        },
    };

    // `WHERE col |` (column already typed, space, cursor): the user wants an operator next.
    if cursor_tok.is_none()
        && let Some(prev) = last_content_before(tokens, cursor)
        && is_predicate_lhs_position(src, tokens, prev)
    {
        return CursorContext::ComparisonOp {
            lhs_col: lhs_col_text(src, tokens, prev),
        };
    }

    classify_by_governing_keyword(src, tokens, scan_from, partial)
}

/// Walk backward from `from` (exclusive of the cursor's own in-progress token) to the nearest
/// governing clause keyword, and classify. Paren-depth aware: a function call wrapping the cursor
/// (`WHERE lower(ci|`) keeps us in the enclosing clause's column position.
fn classify_by_governing_keyword(
    src: &str,
    tokens: &[Token],
    from: usize,
    partial: String,
) -> CursorContext {
    // Walk left to the nearest clause-governing keyword. A function call wrapping the cursor
    // (`WHERE lower(ci|`) is transparent here: the keyword to the left of the call still governs,
    // and the column position inside the call is that clause's column position.
    let mut idx = from;
    while idx > 0 {
        idx -= 1;
        let t = tokens[idx];
        if t.is_trivia() {
            continue;
        }
        if t.kind == TokenKind::Keyword
            && let Some(ctx) = context_for_keyword(src, tokens, idx, t.text(src), from, &partial)
        {
            return ctx;
        }
    }
    // No governing keyword found to the left — a bare keyword position (start of query).
    CursorContext::Keyword { partial }
}

/// Map a governing clause keyword (found while scanning backward) to a context, if it determines
/// one. Returns `None` if this keyword is not clause-governing (so the scan continues left).
///
/// `scan_from` is where the backward walk started (the index just before the cursor's own
/// in-progress token); a predicate keyword consults the tokens between it and `scan_from` to tell
/// a fresh column position from a position where the operand slot is already filled.
fn context_for_keyword(
    src: &str,
    tokens: &[Token],
    kw_idx: usize,
    kw_text: &str,
    scan_from: usize,
    partial: &str,
) -> Option<CursorContext> {
    let kw = kw_text.to_ascii_lowercase();
    match kw.as_str() {
        "select" | "distinct" => Some(CursorContext::SelectList {
            partial: partial.to_string(),
        }),
        // A FROM/JOIN whose relation slot is already filled (a content token sits between this
        // keyword and the cursor's own in-progress token, at the keyword's depth) means the user has
        // moved past the relation and is starting the next clause — `SELECT * FROM t wh|` wants the
        // `WHERE` keyword, not another table. An empty interval (cursor still extending the relation,
        // `SELECT * FROM us|`) keeps us in `FromTable` so partials still filter the table list.
        "from" | "join" => Some(if from_slot_filled(src, tokens, kw_idx, scan_from) {
            CursorContext::Keyword {
                partial: partial.to_string(),
            }
        } else {
            CursorContext::FromTable {
                partial: partial.to_string(),
            }
        }),
        // A predicate-opening keyword. The cursor is at a column (LHS) position *only* when no
        // top-level comparison has already filled the operand slot between this keyword and the
        // cursor. (The `WHERE col |` operator-position case is handled earlier in `detect_context`
        // before this scan.) Once `col op value` / `col IS NULL` / `col IS ` is complete, the user
        // wants a connector/clause keyword — not another column — so we return `Keyword` (§5.4's
        // "after a complete clause where a keyword is expected" row).
        "where" | "and" | "or" | "having" | "on" => {
            Some(match predicate_slot(src, tokens, kw_idx, scan_from) {
                PredicateSlot::Lhs => CursorContext::Predicate {
                    partial: partial.to_string(),
                },
                PredicateSlot::Filled => CursorContext::Keyword {
                    partial: partial.to_string(),
                },
            })
        }
        // `BY` only governs after `GROUP`/`ORDER`; look one content token further left.
        "by" => {
            let prev = prev_content(tokens, kw_idx)?;
            let pt = tokens[prev].text(src).to_ascii_lowercase();
            if pt == "group" || pt == "order" {
                Some(CursorContext::GroupOrderList {
                    partial: partial.to_string(),
                })
            } else {
                None
            }
        }
        // These don't open a column/value position by themselves; keep scanning left.
        "group" | "order" | "as" | "asc" | "desc" | "limit" | "offset" | "in" | "not" | "is"
        | "null" | "like" | "ilike" | "between" => None,
        _ => None,
    }
}

/// Whether the FROM/JOIN at `kw_idx` already has a relation token sitting between it and the
/// cursor's own in-progress token (`scan_from`) at the keyword's paren depth. A filled slot means
/// the user has typed the relation name; the next non-extending token is the start of a new clause,
/// so the cursor sits at a `Keyword` position rather than still extending the table list.
///
/// An empty interval (no content tokens, just whitespace) keeps the cursor in FROM territory — the
/// `SELECT * FROM us|` case where the cursor token IS the relation being typed (so it lives at
/// `scan_from`, not inside this interval).
///
/// A trailing `,` reopens the relation slot (`FROM a, b|` is still typing the second relation), so
/// it doesn't count as filled — the comma is the freshest content token, and the cursor is the
/// start of the next relation.
fn from_slot_filled(src: &str, tokens: &[Token], kw_idx: usize, scan_from: usize) -> bool {
    let kw_depth = tokens[kw_idx].depth;
    let mut filled = false;
    let mut i = kw_idx + 1;
    while i < scan_from {
        let t = tokens[i];
        i += 1;
        if t.is_trivia() || t.depth != kw_depth {
            continue; // ignore trivia and anything nested under a sub-paren
        }
        match t.kind {
            // A relation name (or its qualifier dot) fills the slot.
            TokenKind::Ident | TokenKind::QuotedIdent => filled = true,
            TokenKind::Punct => match t.text(src) {
                // `,` reopens the relation slot for the next entry — clears the fill.
                "," => filled = false,
                // `.` is part of a qualified name (`schema.t`), still filled.
                "." => {}
                // Any other punctuation at top depth (`;`, `*`) shouldn't appear in a FROM list, but
                // if it does, leave the fill state as-is (don't open a hole).
                _ => {}
            },
            // `JOIN` / `AS` keep the slot filled (the previous Ident already filled it; these are
            // structural). Aliases (`AS x`) introduce another Ident which also fills.
            TokenKind::Keyword => {}
            // Numbers / strings have no place in a FROM list; treat conservatively as not changing
            // the fill state so we don't misclassify a half-typed query.
            _ => {}
        }
    }
    filled
}

/// What slot of a predicate the cursor occupies, relative to its governing predicate keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PredicateSlot {
    /// A column (left-hand-side) position — the next token is a column. `WHERE `, `WHERE st`,
    /// `WHERE a = 5 AND ` (after a connector), `WHERE a > ` (dangling operator, a column/value is
    /// still a legal next token).
    Lhs,
    /// The operand slot is already filled (or can only take a connector keyword): a comparison
    /// `col op value` is complete, or `col IS …` has begun (which only takes `NULL`/`NOT NULL`).
    /// A bare column is no longer legal here — the user wants a connector/clause keyword.
    Filled,
}

/// Classify the cursor's position within the predicate governed by the keyword at `kw_idx`, by
/// scanning forward over the tokens between the keyword and `scan_from` (the start of the backward
/// walk) at the keyword's own paren depth.
///
/// The slot becomes [`PredicateSlot::Filled`] once the operand slot is complete or can only take a
/// connector: a comparison `Operator` (or `IN`/`LIKE`/`ILIKE`) completed by an operand, or `IS`
/// (which only takes `NULL`/`NOT NULL`). Otherwise it stays [`PredicateSlot::Lhs`] — a fresh column
/// position. `BETWEEN` and a *dangling* operator are deliberately left as `Lhs`: a column (or value)
/// is still a legal next token there (e.g. `WHERE a BETWEEN ` accepts a column range bound), so v1
/// keeps offering columns. A connector (`AND`/`OR`) reopens the LHS slot for a new sub-predicate.
fn predicate_slot(src: &str, tokens: &[Token], kw_idx: usize, scan_from: usize) -> PredicateSlot {
    let kw_depth = tokens[kw_idx].depth;
    let mut saw_operator = false; // a top-level operator awaiting its operand
    let mut i = kw_idx + 1;
    while i < scan_from {
        let t = tokens[i];
        i += 1;
        // The closing `)` of an `IN (…)` list sits at `kw_depth`; it completes the operand.
        let is_close_paren = t.kind == TokenKind::Punct && t.text(src) == ")";
        if t.depth == kw_depth && is_close_paren && saw_operator {
            return PredicateSlot::Filled;
        }
        if t.is_trivia() || t.depth != kw_depth {
            continue; // skip trivia and anything nested inside a function call / sub-paren
        }
        match t.kind {
            TokenKind::Keyword => match t.text(src).to_ascii_lowercase().as_str() {
                // A connector starts a fresh sub-predicate: the LHS slot reopens.
                "and" | "or" => saw_operator = false,
                // `IS` only takes `NULL`/`NOT NULL` — never a column. The slot is filled the moment
                // it appears (so `WHERE a IS ` offers keywords, not columns).
                "is" => return PredicateSlot::Filled,
                // Membership/pattern operators open a value slot (their operand completes it).
                "in" | "like" | "ilike" => saw_operator = true,
                _ => {}
            },
            // A comparison operator opens the value slot; an operand after it completes the clause.
            TokenKind::Operator => saw_operator = true,
            // Any operand-shaped token after an operator completes the predicate.
            TokenKind::Number
            | TokenKind::StringLit { .. }
            | TokenKind::Ident
            | TokenKind::QuotedIdent
                if saw_operator =>
            {
                return PredicateSlot::Filled;
            }
            _ => {}
        }
    }
    PredicateSlot::Lhs
}

/// Is the content token at `idx` a predicate LHS-complete position (so the next thing is an
/// operator)? True for an `Ident`/`QuotedIdent`/qualified name at top-level paren depth, **or** a
/// closing `)` that completes a function-call expression (`WHERE lower(city) `), when the nearest
/// governing keyword to its left is a predicate keyword (`WHERE`/`AND`/`OR`/`HAVING`/`ON`), with no
/// intervening comparison operator.
fn is_predicate_lhs_position(src: &str, tokens: &[Token], idx: usize) -> bool {
    let t = tokens[idx];
    // A complete function-call LHS ends in a top-level `)`; after it the user wants an operator.
    if t.kind == TokenKind::Punct && t.text(src) == ")" {
        return closes_predicate_call(src, tokens, idx);
    }
    if !matches!(t.kind, TokenKind::Ident | TokenKind::QuotedIdent) {
        return false;
    }
    // Walk left: an operator before any predicate keyword means we're already past the LHS.
    let mut i = idx;
    while i > 0 {
        i -= 1;
        let p = tokens[i];
        if p.is_trivia() {
            continue;
        }
        match p.kind {
            TokenKind::Operator => return false,
            TokenKind::Punct if p.text(src) == "." => continue, // part of `t.col`
            TokenKind::Ident => continue,                       // qualifier of `t.col`
            TokenKind::Keyword => {
                let kw = p.text(src).to_ascii_lowercase();
                if matches!(kw.as_str(), "where" | "and" | "or" | "having" | "on") {
                    return true;
                }
                // LIKE/IN/IS/BETWEEN are operator-ish keywords -> already past LHS.
                if matches!(
                    kw.as_str(),
                    "like" | "ilike" | "in" | "is" | "between" | "not"
                ) {
                    return false;
                }
                // Any other keyword (SELECT/FROM/…) -> not a predicate LHS.
                return false;
            }
            _ => return false,
        }
    }
    false
}

/// The (informational) `lhs_col` to title the `ComparisonOp` popup, for the LHS-complete token at
/// `idx`. A bare/qualified column resolves to its bare name (`t.created_at` -> `created_at`, per
/// §5.7 "strip the `t.` prefix"); a function-call LHS (`lower(city)`, whose token is the closing
/// `)`) has no single column to name, so it yields `None`.
fn lhs_col_text(src: &str, tokens: &[Token], idx: usize) -> Option<String> {
    match tokens[idx].kind {
        TokenKind::Ident | TokenKind::QuotedIdent => Some(column_name_at(src, tokens, idx)),
        _ => None,
    }
}

/// Whether the `)` at `idx` closes a function-call expression in predicate position — i.e. its
/// matching `(` is preceded by an `Ident` (the function name) and the call sits under a predicate
/// keyword with no comparison operator already consumed. This makes `WHERE lower(city) ` a
/// predicate-LHS-complete (operator) position rather than a fresh column position.
fn closes_predicate_call(src: &str, tokens: &[Token], idx: usize) -> bool {
    // The lexer records the depth *inside* a `(` (incremented before push) and the depth *after* a
    // `)` (decremented before push), so the matching `(` of a `)` at depth `d` carries depth `d+1`.
    let close_depth = tokens[idx].depth;
    let open_depth = close_depth + 1;
    let mut open = None;
    let mut i = idx;
    while i > 0 {
        i -= 1;
        let t = tokens[i];
        if t.is_trivia() {
            continue;
        }
        if t.kind == TokenKind::Punct && t.text(src) == "(" && t.depth == open_depth {
            open = Some(i);
            break;
        }
        // A top-level comparison operator before the matching `(` means we are past the LHS.
        if t.depth == close_depth && t.kind == TokenKind::Operator {
            return false;
        }
    }
    let Some(open) = open else { return false };
    // The token just left of the `(` must be the function name (an Ident), e.g. `lower(`.
    let Some(name_idx) = prev_content(tokens, open) else {
        return false;
    };
    if tokens[name_idx].kind != TokenKind::Ident {
        return false;
    }
    // And that call must sit directly under a predicate keyword (no operator already consumed).
    is_predicate_lhs_position(src, tokens, name_idx)
}

/// Resolve the bare column name for a column reference token at `idx`, stripping any `qualifier.`
/// prefix and any surrounding double-quotes. `t.created_at` -> `created_at`; `"order"` -> `order`.
fn column_name_at(src: &str, tokens: &[Token], idx: usize) -> String {
    // A qualified `t.created_at` lexes as `t` `.` `created_at`; the column token IS the trailing
    // `created_at`, so its own text is already the bare name (the qualifier is a separate token).
    let t = tokens[idx];
    let raw = t.text(src);
    match t.kind {
        TokenKind::QuotedIdent => unquote_ident(raw),
        _ => raw.to_string(),
    }
}

/// Strip surrounding double-quotes from a `"quoted ident"`, collapsing the `""` escape to `"`.
fn unquote_ident(raw: &str) -> String {
    let inner = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(raw);
    inner.replace("\"\"", "\"")
}

/// Index of the content token immediately before `idx` (skipping trivia).
fn prev_content(tokens: &[Token], idx: usize) -> Option<usize> {
    let mut i = idx;
    while i > 0 {
        i -= 1;
        if !tokens[i].is_trivia() {
            return Some(i);
        }
    }
    None
}

/// Index of the last content token whose span ends at or before `cursor`.
fn last_content_before(tokens: &[Token], cursor: usize) -> Option<usize> {
    let mut found = None;
    for (idx, t) in tokens.iter().enumerate() {
        if t.is_trivia() {
            continue;
        }
        if t.end <= cursor {
            found = Some(idx);
        } else {
            break;
        }
    }
    found
}

/// If the cursor sits inside an **open** string literal, return that token's index. A closed
/// literal contributes no value-mode (the cursor is past a complete value).
fn open_string_literal_at(tokens: &[Token], cursor: usize) -> Option<usize> {
    for (idx, t) in tokens.iter().enumerate() {
        if let TokenKind::StringLit { closed: false } = t.kind
            && t.start < cursor
            && cursor <= t.end
        {
            return Some(idx);
        }
    }
    None
}

/// Given the index of an open string literal, resolve the `(column, TriggerKind)` it's a value for,
/// by walking left over the operator and the column. Handles `col = '`, `col != '`, `col LIKE '`,
/// `col IN ('`, and ordered comparisons.
fn value_trigger_before(
    src: &str,
    tokens: &[Token],
    lit_idx: usize,
) -> Option<(String, TriggerKind)> {
    // The token immediately left of the literal (skipping trivia and an opening `(` for IN-lists).
    let mut i = lit_idx;
    // Skip the structural tokens of an IN-list: the opening `(`, the element separators `,`, and
    // any already-listed elements (closed string literals / numbers) — `col IN ('a', 'b', '`. The
    // cursor literal follows them; the operator we want is `IN` to their left.
    let op_idx = loop {
        let p = prev_content(tokens, i)?;
        let skip = match tokens[p].kind {
            TokenKind::Punct => matches!(tokens[p].text(src), "(" | ","),
            TokenKind::StringLit { .. } | TokenKind::Number => true,
            _ => false,
        };
        if skip {
            i = p;
            continue;
        }
        break p;
    };

    let op_tok = tokens[op_idx];
    let op_text = op_tok.text(src);

    // Case A: an `Operator` token (`=`, `!=`, `<>`, `<`, `<=`, `>`, `>=`).
    if op_tok.kind == TokenKind::Operator {
        let kind = match op_text {
            "=" => TriggerKind::Eq,
            "!=" | "<>" => TriggerKind::Neq,
            _ => TriggerKind::Cmp,
        };
        let col_idx = prev_content(tokens, op_idx)?;
        let col = resolve_column(src, tokens, col_idx)?;
        return Some((col, kind));
    }

    // Case B: a keyword operator (`LIKE`/`ILIKE`/`IN`).
    if op_tok.kind == TokenKind::Keyword {
        let kw = op_text.to_ascii_lowercase();
        let (kind, col_idx) = match kw.as_str() {
            "like" | "ilike" => (TriggerKind::Like, prev_content(tokens, op_idx)?),
            "in" => (TriggerKind::In, prev_content(tokens, op_idx)?),
            _ => return None,
        };
        let col = resolve_column(src, tokens, col_idx)?;
        return Some((col, kind));
    }

    None
}

/// Resolve a column reference at `idx` to its bare name, if `idx` is actually a column-shaped
/// token (`Ident` or `QuotedIdent`). Returns `None` for anything else (e.g. a number, so
/// `5 = 'x'` doesn't pretend `5` is a column).
fn resolve_column(src: &str, tokens: &[Token], idx: usize) -> Option<String> {
    match tokens[idx].kind {
        TokenKind::Ident | TokenKind::QuotedIdent => Some(column_name_at(src, tokens, idx)),
        _ => None,
    }
}

#[cfg(test)]
#[path = "clause_context_tests.rs"]
mod clause_context_tests;
