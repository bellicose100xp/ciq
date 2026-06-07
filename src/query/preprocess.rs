//! Query preprocessing — validate the interactive grammar and apply the viewport LIMIT wrap.
//!
//! `dev/PLAN.md` §2.3 + §0 (Q1 restricted grammar). Interactive queries must be a **single,
//! read-only `SELECT`** (optionally a leading `WITH … SELECT` CTE). This module:
//!  - rejects multi-statement input and non-SELECT/DML (`INSERT`/`UPDATE`/`COPY`/`PRAGMA`/…),
//!    so the resident table `t` is never mutated and every keystroke is idempotent;
//!  - strips a single trailing `;`;
//!  - wraps the query to cap rows at the viewport budget, but **only when the user supplied no
//!    top-level `LIMIT`** — an existing `LIMIT k` (incl. `ORDER BY … LIMIT k`) is respected and
//!    never doubled.
//!
//! All three checks are built on **one shared scan** — `crate::sql_lexer::tokenize` (`dev/PLAN.md`
//! §5.3, `dev/DECISIONS.md` D6) — that correctly handles single-quoted strings (`'...'`, `''`
//! escape), double-quoted identifiers (`"..."`, `""` escape), `--` line comments, `/* */` block
//! comments, and paren depth. Per D6's binding forward-rule, preprocess **consumes that shared
//! lexer** rather than carrying its own tokenizer; the read-only-grammar checks are derived from
//! the resulting `&[Token]`. Pure `&str -> Result<String, PreprocessError>`; table-driven tested.

use crate::sql_lexer::{Token, TokenKind, tokenize};

/// Why an interactive query was rejected before reaching the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreprocessError {
    /// The input was empty / whitespace-only (or only comments).
    Empty,
    /// More than one statement (a top-level `;` with content after it).
    MultipleStatements,
    /// Not a read-only `SELECT` / `WITH … SELECT`.
    NotReadOnly,
}

impl PreprocessError {
    /// A short, user-facing status-line message.
    pub fn message(&self) -> &'static str {
        match self {
            PreprocessError::Empty => "empty query",
            PreprocessError::MultipleStatements => "single statement only",
            PreprocessError::NotReadOnly => "read-only SELECT queries only",
        }
    }
}

/// Validate the interactive grammar and wrap to the viewport `limit`.
///
/// On success returns the exact SQL to send the engine. On rejection returns a
/// `PreprocessError` (surfaced in the status line; no engine call is issued).
pub fn prepare_interactive(input: &str, limit: usize) -> Result<String, PreprocessError> {
    let tokens = tokenize(input);

    // Reject a top-level `;` that has any real token after it (multiple statements). A single
    // trailing `;` with nothing meaningful after it is fine. `;` is detected regardless of paren
    // balance (the lexer always emits it), so a stray `)`/`(` can't smuggle a second statement.
    if let Some(semi) = tokens.iter().find(|t| is_semicolon(input, t)) {
        let has_second_statement = tokens
            .iter()
            .any(|t| t.start > semi.start && is_meaningful(input, t));
        if has_second_statement {
            return Err(PreprocessError::MultipleStatements);
        }
    }

    // Leading keyword decides read-only-ness. Use the first *word* (keyword or identifier) at any
    // depth so a leading `(` (parenthesized SELECT) doesn't hide the keyword.
    let lead = match tokens.iter().find(|t| is_word(t)) {
        Some(t) => t,
        None => return Err(PreprocessError::Empty), // empty / only comments / only punctuation
    };
    let lead_text = lead.text(input);
    if !(lead_text.eq_ignore_ascii_case("SELECT") || lead_text.eq_ignore_ascii_case("WITH")) {
        return Err(PreprocessError::NotReadOnly);
    }
    // A bare `SELECT`/`WITH` with nothing meaningful after it is not a runnable statement.
    let has_body = tokens
        .iter()
        .any(|t| t.start > lead.start && is_meaningful(input, t));
    if !has_body {
        return Err(PreprocessError::NotReadOnly);
    }

    // Normalize the statement to send the engine: rebuild from the source span up to (and
    // excluding) any trailing top-level `;`, so a trailing comment can't swallow our wrapper.
    let normalized = normalized_sql(input, &tokens);

    if has_top_level_limit(input, &tokens) {
        // Respect the user's own LIMIT — do not wrap or double it.
        Ok(normalized)
    } else {
        // Wrap so a bare `SELECT *` returns a screenful, not the whole table. The subquery
        // preserves the user's own ORDER BY ordering; the outer LIMIT caps to the viewport.
        // Newlines around the subquery guard against a trailing `--` line comment swallowing
        // the `) AS _ciq LIMIT n` we append.
        Ok(format!(
            "SELECT * FROM (\n{normalized}\n) AS _ciq LIMIT {limit}"
        ))
    }
}

/// The source SQL with comments preserved but any trailing top-level `;` (and everything that
/// would be only whitespace after it) removed. We rebuild from the original byte span so the
/// engine sees the user's exact text (formatting, comments) minus the statement terminator.
fn normalized_sql(input: &str, tokens: &[Token]) -> String {
    let end = match tokens.iter().find(|t| is_semicolon(input, t)) {
        Some(semi) => semi.start,
        None => input.len(),
    };
    input[..end].trim().to_string()
}

/// Whether the query has a top-level (`depth == 0`) `LIMIT` clause (so we must not wrap).
/// Scans *all* depth-0 keyword tokens (not a fixed tail window), avoiding both the "LIMIT pushed
/// out of a short window" miss and the `OFFSET`-after-LIMIT case. A `limit` written as a quoted
/// identifier (`"limit"`) lexes as `QuotedIdent`, not `Keyword`, so it doesn't false-positive. A
/// `limit` nested in a subquery has `depth > 0`, so it doesn't count as the outer clause.
fn has_top_level_limit(input: &str, tokens: &[Token]) -> bool {
    tokens.iter().any(|t| {
        t.kind == TokenKind::Keyword && t.depth == 0 && t.text(input).eq_ignore_ascii_case("LIMIT")
    })
}

/// Whether a token is the statement terminator `;` (a one-byte `Punct` with that text). The lexer
/// always emits `;` regardless of paren depth, so this is the fail-closed multi-statement signal.
fn is_semicolon(input: &str, t: &Token) -> bool {
    t.kind == TokenKind::Punct && t.text(input) == ";"
}

/// Whether a token is "word-shaped" — an unquoted keyword or identifier. The leading-keyword
/// check keys off the first such token (so a leading `(` or comment can't hide the SELECT/WITH).
fn is_word(t: &Token) -> bool {
    matches!(t.kind, TokenKind::Keyword | TokenKind::Ident)
}

/// Whether a token is real statement content (vs trivia or a bare `;` separator). Used to decide
/// "is there a second statement after `;`" and "does the leading keyword have a body".
fn is_meaningful(input: &str, t: &Token) -> bool {
    !t.is_trivia() && !is_semicolon(input, t)
}

#[cfg(test)]
#[path = "preprocess_tests.rs"]
mod preprocess_tests;
