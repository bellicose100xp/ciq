//! Shared SQL lexer — one tolerant, total `tokenize(&str) -> Vec<Token>` scan that both the
//! `query` preprocessor and (Phase 3) `autocomplete` consume. A SQL-domain primitive: it lives
//! at the crate top level (sibling of `schema.rs`), so neither subsystem depends on the other.
//!
//! This subsumes the three hand-rolled scanners the first cut of `query/preprocess.rs` shipped
//! (see `dev/DECISIONS.md` D6): there is exactly one place that tracks string/comment/quote
//! state and paren depth. `dev/PLAN.md` §5.3 lists this under `autocomplete/`; that placement is
//! illustrative — the neutral top-level home is what lets `query` import it without a
//! `query -> autocomplete` dependency inversion (D6's "promote to a shared module both import").
//!
//! Two hard guarantees the downstream safety check (statement-smuggling guard) and the
//! mid-keystroke autocomplete both rely on:
//!  - **Tolerant of half-typed input** — an unterminated `'New`, a trailing `WHERE col =`, or an
//!    unbalanced `(` is a normal state, never an error. `StringLit` records whether its closing
//!    quote was seen so callers can tell "in a value literal" from "literal complete".
//!  - **Total — never panics on arbitrary bytes.** The scan iterates by `char` and only ever
//!    slices at boundaries it computed from char widths, so multi-byte input (`¡`, `日本`) can
//!    never split a code point. Guarded by a `proptest` + the committed `proptest-regressions/`
//!    seed that first caught a real panic on `"¡"`.
//!
//! Not a parser: it builds no AST. It tracks per-token byte spans and a running paren depth (the
//! one idea kept from jiq's `brace_tracker`), which is all the restricted single-`SELECT` grammar
//! and the clause-context detector need.

/// What a token is. A superset covering both consumers: the preprocessor's read-only-grammar
/// checks and autocomplete's clause-context detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A reserved SQL keyword (`SELECT`, `FROM`, `WHERE`, `LIMIT`, …), case-insensitive. The
    /// lexer classifies an unquoted word as `Keyword` iff it is in the shared keyword set;
    /// everything else unquoted-and-word-shaped is `Ident`.
    Keyword,
    /// An unquoted identifier (a column/table/alias name that is not a keyword).
    Ident,
    /// A double-quoted identifier, e.g. `"order"` — quotes included in the span. Never matches a
    /// keyword, so a column literally named `"limit"`/`"order"` is safe. `""` is an escaped quote.
    QuotedIdent,
    /// A numeric literal (`42`, `3.14`, `1e9`). Lexed leniently — half-typed numbers are fine.
    Number,
    /// A single-quoted string literal. `closed` is false while the user is still typing inside it
    /// (`'New` mid-keystroke); `''` is an escaped quote that stays inside the literal.
    StringLit { closed: bool },
    /// A comparison/assignment operator: `=`, `!=`, `<>`, `<`, `<=`, `>`, `>=`.
    Operator,
    /// Punctuation: `,` `(` `)` `.` `*` `;`. (`;` is identified by its one-byte text.)
    Punct,
    /// A `--` line comment or a `/* */` block comment (unterminated block comment runs to EOF).
    Comment,
    /// A run of ASCII whitespace.
    Whitespace,
}

/// One token: its kind, its byte span `start..end` into the source, and the paren `depth` in
/// effect at its start. `depth` is clamped at 0 (a stray `)` can never drive it negative), so a
/// top-level (`depth == 0`) `;` or `LIMIT` is detected regardless of paren balance — the
/// fail-closed property the statement-smuggling guard depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
    pub depth: i32,
}

impl Token {
    /// The token's source text.
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        &src[self.start..self.end]
    }

    /// Whether this token is skippable layout (whitespace or a comment) rather than content.
    pub fn is_trivia(&self) -> bool {
        matches!(self.kind, TokenKind::Whitespace | TokenKind::Comment)
    }
}

/// Tokenize `src` into a gap-free, in-order sequence of tokens whose spans tile the entire input
/// (`concat(tok.text for tok in tokens) == src`). Total and tolerant: never panics, never errors;
/// half-typed input yields tokens in their natural in-progress state.
pub fn tokenize(src: &str) -> Vec<Token> {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut out = Vec::new();
    let mut i = 0;
    let mut depth: i32 = 0;

    while i < n {
        let start = i;
        let c = bytes[i];
        match c {
            b'\'' => {
                // String literal: skip to the closing `'` honoring the `''` escape. The depth at
                // its start is recorded even though strings can't change depth.
                let (end, closed) = scan_string(bytes, i);
                out.push(Token {
                    kind: TokenKind::StringLit { closed },
                    start,
                    end,
                    depth,
                });
                i = end;
            }
            b'"' => {
                let (end, _closed) = scan_string_dq(bytes, i);
                out.push(Token {
                    kind: TokenKind::QuotedIdent,
                    start,
                    end,
                    depth,
                });
                i = end;
            }
            b'-' if i + 1 < n && bytes[i + 1] == b'-' => {
                // Line comment to end-of-line (the newline is not part of the comment).
                i += 2;
                while i < n && bytes[i] != b'\n' {
                    i += 1;
                }
                out.push(Token {
                    kind: TokenKind::Comment,
                    start,
                    end: i,
                    depth,
                });
            }
            b'/' if i + 1 < n && bytes[i + 1] == b'*' => {
                // Block comment to the matching `*/`, or EOF if unterminated (half-typed).
                i += 2;
                while i + 1 < n && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = if i + 1 < n { i + 2 } else { n };
                out.push(Token {
                    kind: TokenKind::Comment,
                    start,
                    end: i,
                    depth,
                });
            }
            b'(' => {
                depth += 1;
                i += 1;
                out.push(Token {
                    kind: TokenKind::Punct,
                    start,
                    end: i,
                    depth,
                });
            }
            b')' => {
                // Clamp at 0 so a stray `)` can never push depth negative — see `Token::depth`.
                depth = (depth - 1).max(0);
                i += 1;
                out.push(Token {
                    kind: TokenKind::Punct,
                    start,
                    end: i,
                    depth,
                });
            }
            b',' | b'.' | b'*' | b';' => {
                i += 1;
                out.push(Token {
                    kind: TokenKind::Punct,
                    start,
                    end: i,
                    depth,
                });
            }
            b'=' => {
                i += 1;
                out.push(Token {
                    kind: TokenKind::Operator,
                    start,
                    end: i,
                    depth,
                });
            }
            b'!' if i + 1 < n && bytes[i + 1] == b'=' => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::Operator,
                    start,
                    end: i,
                    depth,
                });
            }
            b'<' => {
                // `<`, `<=`, or `<>`.
                i += 1;
                if i < n && (bytes[i] == b'=' || bytes[i] == b'>') {
                    i += 1;
                }
                out.push(Token {
                    kind: TokenKind::Operator,
                    start,
                    end: i,
                    depth,
                });
            }
            b'>' => {
                // `>` or `>=`.
                i += 1;
                if i < n && bytes[i] == b'=' {
                    i += 1;
                }
                out.push(Token {
                    kind: TokenKind::Operator,
                    start,
                    end: i,
                    depth,
                });
            }
            _ if c.is_ascii_digit() => {
                // Lenient numeric literal: digits, a single-ish `.`, and exponent chars. We do not
                // validate the number — half-typed `3.` / `1e` are fine.
                i += 1;
                while i < n
                    && (bytes[i].is_ascii_digit()
                        || bytes[i] == b'.'
                        || bytes[i] == b'e'
                        || bytes[i] == b'E'
                        || ((bytes[i] == b'+' || bytes[i] == b'-')
                            && (bytes[i - 1] == b'e' || bytes[i - 1] == b'E')))
                {
                    i += 1;
                }
                out.push(Token {
                    kind: TokenKind::Number,
                    start,
                    end: i,
                    depth,
                });
            }
            _ if c.is_ascii_alphabetic() || c == b'_' => {
                // Unquoted word: keyword or identifier.
                i += 1;
                while i < n && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let kind = if is_keyword(&src[start..i]) {
                    TokenKind::Keyword
                } else {
                    TokenKind::Ident
                };
                out.push(Token {
                    kind,
                    start,
                    end: i,
                    depth,
                });
            }
            _ if c.is_ascii_whitespace() => {
                i += 1;
                while i < n && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                out.push(Token {
                    kind: TokenKind::Whitespace,
                    start,
                    end: i,
                    depth,
                });
            }
            _ => {
                // Any other byte: ASCII punctuation we don't model, or a multi-byte non-ASCII char
                // (`¡`, `日`). Emit one full UTF-8 char as `Punct` so the span never splits a code
                // point — `src[i..]` is valid UTF-8 here, so the leading char width is exact.
                let ch_len = src[i..].chars().next().map_or(1, char::len_utf8);
                i += ch_len;
                out.push(Token {
                    kind: TokenKind::Punct,
                    start,
                    end: i,
                    depth,
                });
            }
        }
    }

    out
}

/// From the opening `'` at `open`, return `(end, closed)` where `end` is the index just past the
/// closing `'` (honoring the `''` escape) and `closed` says whether a closing quote was found.
/// Unterminated (`closed == false`) returns the input length — a normal mid-keystroke state.
fn scan_string(bytes: &[u8], open: usize) -> (usize, bool) {
    let n = bytes.len();
    let mut i = open + 1;
    while i < n {
        if bytes[i] == b'\'' {
            if i + 1 < n && bytes[i + 1] == b'\'' {
                i += 2; // escaped quote, stay inside
                continue;
            }
            return (i + 1, true);
        }
        i += 1;
    }
    (n, false)
}

/// Same as [`scan_string`] but for a double-quoted identifier (`""` escape). The `closed` flag is
/// returned for symmetry; quoted idents don't distinguish it downstream.
fn scan_string_dq(bytes: &[u8], open: usize) -> (usize, bool) {
    let n = bytes.len();
    let mut i = open + 1;
    while i < n {
        if bytes[i] == b'"' {
            if i + 1 < n && bytes[i + 1] == b'"' {
                i += 2;
                continue;
            }
            return (i + 1, true);
        }
        i += 1;
    }
    (n, false)
}

/// The reserved-keyword set the lexer recognizes (case-insensitive). Kept deliberately small and
/// focused on the restricted read-only grammar plus the clause keywords the context detector
/// walks back to (P3.2). The exhaustive DuckDB function/keyword *candidate* tables for the popup
/// are a separate concern (P3.3's `sql_keywords.rs`); this set only decides `Keyword` vs `Ident`.
fn is_keyword(word: &str) -> bool {
    KEYWORDS.iter().any(|kw| word.eq_ignore_ascii_case(kw))
}

const KEYWORDS: &[&str] = &[
    "select",
    "from",
    "where",
    "group",
    "by",
    "order",
    "having",
    "limit",
    "offset",
    "join",
    "inner",
    "left",
    "right",
    "full",
    "outer",
    "cross",
    "on",
    "using",
    "with",
    "as",
    "and",
    "or",
    "not",
    "in",
    "is",
    "null",
    "like",
    "ilike",
    "between",
    "asc",
    "desc",
    "distinct",
    "union",
    "all",
    "except",
    "intersect",
    "case",
    "when",
    "then",
    "else",
    "end",
    "exists",
    "into",
];

/// Find the index of the token whose span contains byte `cursor`, preferring the token *ending*
/// at the cursor (so a cursor just past the last typed char is "inside" the token being typed).
/// Returns `None` only for the empty/whitespace-cursor case where no content token applies.
///
/// Convention: a cursor at position `p` belongs to the token `t` with `t.start < p <= t.end`
/// (the token being extended by the next keystroke), falling back to `t.start <= p < t.end` for a
/// cursor resting at a token's start. Trivia (whitespace/comments) are skipped so the cursor binds
/// to adjacent content, which is what the clause-context detector wants.
pub fn token_at_cursor(tokens: &[Token], cursor: usize) -> Option<usize> {
    // First preference: a content token the cursor is extending (cursor == end, or strictly
    // inside). This is the in-progress partial.
    for (idx, t) in tokens.iter().enumerate() {
        if t.is_trivia() {
            continue;
        }
        if t.start < cursor && cursor <= t.end {
            return Some(idx);
        }
    }
    // Fallback: a content token whose start the cursor rests exactly on.
    for (idx, t) in tokens.iter().enumerate() {
        if !t.is_trivia() && t.start == cursor {
            return Some(idx);
        }
    }
    None
}

/// The in-progress token text immediately *before* `cursor` — the `partial` the clause-context
/// detector (P3.2) and the candidate fuzzy-filter need. For a word/ident/keyword/quoted-ident or
/// an open string literal, returns the portion from the token start up to the cursor; otherwise
/// (cursor on whitespace, punctuation, or a closed literal) returns `""`.
///
/// This is a pure helper over the token stream and the cursor — no I/O, total for any `cursor` in
/// `0..=src.len()`.
pub fn partial_at_cursor(src: &str, tokens: &[Token], cursor: usize) -> String {
    let Some(idx) = token_at_cursor(tokens, cursor) else {
        return String::new();
    };
    let t = tokens[idx];
    let cursor = cursor.min(t.end);
    match t.kind {
        TokenKind::Ident | TokenKind::Keyword | TokenKind::Number => {
            src[t.start..cursor].to_string()
        }
        // Quoted ident: the partial is the inner text typed so far, sans the leading `"`.
        TokenKind::QuotedIdent => {
            let inner_start = (t.start + 1).min(cursor);
            src[inner_start..cursor].to_string()
        }
        // Open string literal: the partial is the value typed so far, sans the leading `'`. A
        // closed literal contributes no partial (the cursor is past a complete value).
        TokenKind::StringLit { closed } => {
            if closed {
                String::new()
            } else {
                let inner_start = (t.start + 1).min(cursor);
                src[inner_start..cursor].to_string()
            }
        }
        _ => String::new(),
    }
}

#[cfg(test)]
#[path = "sql_lexer_tests.rs"]
mod sql_lexer_tests;
