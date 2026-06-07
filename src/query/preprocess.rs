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
//! All three checks are built on **one shared `top_level_tokens` scan** that correctly handles
//! single-quoted strings (`'...'`, `''` escape), double-quoted identifiers (`"..."`, `""`
//! escape), `--` line comments, `/* */` block comments, and paren depth. This is a small,
//! deliberate tokenizer-lite (§5.3 "tokenizer, not parser"); when P3.1 lands the full
//! `src/autocomplete/sql_lexer.rs`, preprocess should consume that lexer instead of this local
//! scan. Pure `&str -> Result<String, PreprocessError>`; table-driven tested.

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
    let tokens = top_level_tokens(input);

    // Reject a top-level `;` that has any real token after it (multiple statements). A single
    // trailing `;` with nothing meaningful after it is fine.
    if let Some(semi) = tokens.iter().find(|t| t.kind == TokKind::Semicolon) {
        let has_second_statement = tokens
            .iter()
            .any(|t| t.start > semi.start && t.is_meaningful());
        if has_second_statement {
            return Err(PreprocessError::MultipleStatements);
        }
    }

    // Leading keyword decides read-only-ness. Use the first *word* at any depth so a leading
    // `(` (parenthesized SELECT) doesn't hide the keyword.
    let lead = match tokens.iter().find(|t| t.kind == TokKind::Word) {
        Some(t) => t,
        None => return Err(PreprocessError::Empty), // empty / only comments / only punctuation
    };
    if !(lead.text.eq_ignore_ascii_case("SELECT") || lead.text.eq_ignore_ascii_case("WITH")) {
        return Err(PreprocessError::NotReadOnly);
    }
    // A bare `SELECT`/`WITH` with nothing meaningful after it is not a runnable statement.
    let has_body = tokens
        .iter()
        .any(|t| t.start > lead.start && t.is_meaningful());
    if !has_body {
        return Err(PreprocessError::NotReadOnly);
    }

    // Normalize the statement to send the engine: rebuild from the source span up to (and
    // excluding) any trailing top-level `;`, so a trailing comment can't swallow our wrapper.
    let normalized = normalized_sql(input, &tokens);

    if has_top_level_limit(&tokens) {
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
fn normalized_sql(input: &str, tokens: &[Tok]) -> String {
    let end = match tokens.iter().find(|t| t.kind == TokKind::Semicolon) {
        Some(semi) => semi.start,
        None => input.len(),
    };
    input[..end].trim().to_string()
}

/// Whether the query has a top-level (`depth == 0`) `LIMIT` clause (so we must not wrap).
/// Scans *all* depth-0 word tokens (not a fixed tail window), avoiding both the "LIMIT pushed
/// out of a short window" miss and the `OFFSET`-after-LIMIT case. A `limit` written as a
/// quoted identifier (`"limit"`) is a `QuotedIdent`, not a `Word`, so it doesn't false-positive.
/// A `limit` nested in a subquery has `depth > 0`, so it doesn't count as the outer clause.
fn has_top_level_limit(tokens: &[Tok]) -> bool {
    tokens
        .iter()
        .any(|t| t.kind == TokKind::Word && t.depth == 0 && t.text.eq_ignore_ascii_case("LIMIT"))
}

/// Token kind for the restricted-grammar scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokKind {
    /// An unquoted identifier or keyword.
    Word,
    /// A statement terminator (only emitted at `depth == 0`).
    Semicolon,
    /// A double-quoted identifier (`"order"`). Never matches keywords.
    QuotedIdent,
    /// A run of other punctuation/operator bytes (not whitespace/comment/string).
    Other,
}

/// A token from the scan: kind, source text, byte start, and paren depth at its position.
#[derive(Debug, Clone)]
struct Tok<'a> {
    kind: TokKind,
    text: &'a str,
    start: usize,
    depth: i32,
}

impl Tok<'_> {
    /// Whether this token represents real statement content (vs a bare separator). Used to
    /// decide "is there a second statement after `;`" and "does the leading keyword have a body".
    fn is_meaningful(&self) -> bool {
        matches!(
            self.kind,
            TokKind::Word | TokKind::QuotedIdent | TokKind::Other
        )
    }
}

/// Single shared scan producing the tokens all three checks consume (no duplicated scanners).
/// Correctly skips `'...'` strings (with `''` escape), emits `"..."` quoted idents (with `""`
/// escape), and skips `--` line and `/* */` block comments. Each token carries its paren
/// `depth` so callers filter to top-level where the grammar requires it (LIMIT clause, `;`
/// terminator) while still seeing nested words (e.g. a leading parenthesized `SELECT`). Pure,
/// total, never panics; slices only at byte boundaries it controls.
///
/// NOTE (P3.1): when the full `src/autocomplete/sql_lexer.rs` lands it should SUBSUME this scan
/// (same string/comment/quote/paren handling) rather than duplicate it — `Tok`/`TokKind` are
/// the seed of that shared lexer.
fn top_level_tokens(s: &str) -> Vec<Tok<'_>> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut depth: i32 = 0;
    let n = bytes.len();

    while i < n {
        let c = bytes[i];
        match c {
            b'\'' => i = skip_quoted(bytes, i, b'\''), // string literal — skip entirely
            b'"' => {
                let end = skip_quoted(bytes, i, b'"');
                out.push(Tok {
                    kind: TokKind::QuotedIdent,
                    text: &s[i..end],
                    start: i,
                    depth,
                });
                i = end;
            }
            b'-' if i + 1 < n && bytes[i + 1] == b'-' => {
                // line comment: skip to end of line (or end of input)
                i += 2;
                while i < n && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < n && bytes[i + 1] == b'*' => {
                // block comment: skip to matching `*/` (or end of input)
                i += 2;
                while i + 1 < n && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(n);
            }
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                // Clamp at 0: a stray `)` must never drive depth negative, or a later
                // top-level `;`/`LIMIT` (which key off depth) would be silently missed —
                // a statement-smuggling / wrap-bypass hole (caught by the fix re-review).
                depth = (depth - 1).max(0);
                i += 1;
            }
            b';' => {
                // A `;` outside a string/comment is ALWAYS a statement terminator for the
                // safety (multi-statement) check, regardless of paren depth. Failing closed
                // on malformed/unbalanced input is the safe choice: the resident table `t`
                // must never be mutated. `depth` is recorded for callers that care.
                out.push(Tok {
                    kind: TokKind::Semicolon,
                    text: ";",
                    start: i,
                    depth,
                });
                i += 1;
            }
            _ if c.is_ascii_alphanumeric() || c == b'_' => {
                let start = i;
                while i < n && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                out.push(Tok {
                    kind: TokKind::Word,
                    text: &s[start..i],
                    start,
                    depth,
                });
            }
            _ if c.is_ascii_whitespace() => i += 1,
            _ => {
                // Any other char (ASCII punctuation/operator, OR a multi-byte non-ASCII char
                // such as `¡` or `日`). Advance by the FULL UTF-8 char width so the slice below
                // never splits a code point — `c` is a byte, but `s[i..]` is valid UTF-8 here.
                let start = i;
                let ch_len = s[i..].chars().next().map_or(1, char::len_utf8);
                i += ch_len;
                out.push(Tok {
                    kind: TokKind::Other,
                    text: &s[start..i],
                    start,
                    depth,
                });
            }
        }
    }
    out
}

/// From an opening quote byte at `open`, return the index just past the closing quote of the
/// same kind, honoring the doubled-quote escape (`''` / `""`). If unterminated (half-typed),
/// returns `n` — a normal mid-keystroke state, not an error.
fn skip_quoted(bytes: &[u8], open: usize, q: u8) -> usize {
    let n = bytes.len();
    let mut i = open + 1;
    while i < n {
        if bytes[i] == q {
            if i + 1 < n && bytes[i + 1] == q {
                i += 2; // escaped quote, stay inside
                continue;
            }
            return i + 1; // closing quote
        }
        i += 1;
    }
    n // unterminated
}

#[cfg(test)]
#[path = "preprocess_tests.rs"]
mod preprocess_tests;
