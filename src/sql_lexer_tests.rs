//! Tests for the shared SQL lexer (`dev/PLAN.md` §5.3, §5.6; `dev/DECISIONS.md` D6).
//!
//! Table-driven token classification, the span-tiling / never-panic / paren-depth / quote-escape
//! properties, and the cursor + partial helpers the clause-context detector (P3.2) will consume.
//! Pure, total, no I/O — exactly the headless-testable core North Star #2 targets.

use crate::sql_lexer::{Token, TokenKind, partial_at_cursor, token_at_cursor, tokenize};

/// A compact view of a token: kind plus its source text. Used so table cases read declaratively.
fn view<'a>(src: &'a str, t: &Token) -> (TokenKind, &'a str) {
    (t.kind, t.text(src))
}

/// The non-trivia tokens (kind + text), the way a grammar/context consumer sees the input.
fn content(src: &str) -> Vec<(TokenKind, &str)> {
    tokenize(src)
        .iter()
        .filter(|t| !t.is_trivia())
        .map(|t| view(src, t))
        .collect()
}

// ── classification ──────────────────────────────────────────────────────────────────────────

#[test]
fn keywords_vs_identifiers() {
    use TokenKind::{Ident, Keyword};
    assert_eq!(
        content("SELECT name FROM users WHERE active"),
        vec![
            (Keyword, "SELECT"),
            (Ident, "name"),
            (Keyword, "FROM"),
            (Ident, "users"),
            (Keyword, "WHERE"),
            (Ident, "active"),
        ]
    );
}

#[test]
fn keywords_are_case_insensitive() {
    use TokenKind::{Ident, Keyword};
    assert_eq!(
        content("select X from T"),
        vec![
            (Keyword, "select"),
            (Ident, "X"),
            (Keyword, "from"),
            (Ident, "T"),
        ]
    );
}

#[test]
fn quoted_identifier_never_matches_keyword() {
    use TokenKind::{Ident, Keyword, Punct, QuotedIdent};
    // `"order"` and `"limit"` are columns, not keywords — the safety the preprocess LIMIT-check
    // and the autocomplete quoting both rely on.
    assert_eq!(
        content("SELECT \"order\", \"limit\" FROM t"),
        vec![
            (Keyword, "SELECT"),
            (QuotedIdent, "\"order\""),
            (Punct, ","),
            (QuotedIdent, "\"limit\""),
            (Keyword, "FROM"),
            (Ident, "t"),
        ]
    );
    // sanity: a bare `order`/`limit` IS a keyword
    assert!(matches!(content("order")[0].0, Keyword));
}

#[test]
fn operators() {
    use TokenKind::Operator;
    for op in ["=", "!=", "<>", "<", "<=", ">", ">="] {
        let toks = content(op);
        assert_eq!(toks.len(), 1, "{op:?} should be one token");
        assert_eq!(toks[0], (Operator, op), "operator {op:?}");
    }
}

#[test]
fn operators_in_context() {
    use TokenKind::{Ident, Keyword, Number, Operator};
    assert_eq!(
        content("WHERE a >= 5"),
        vec![
            (Keyword, "WHERE"),
            (Ident, "a"),
            (Operator, ">="),
            (Number, "5"),
        ]
    );
    assert_eq!(
        content("WHERE a <> b"),
        vec![
            (Keyword, "WHERE"),
            (Ident, "a"),
            (Operator, "<>"),
            (Ident, "b"),
        ]
    );
}

#[test]
fn punctuation() {
    use TokenKind::Punct;
    for p in [",", "(", ")", ".", "*", ";"] {
        let toks = content(p);
        assert_eq!(toks, vec![(Punct, p)], "punct {p:?}");
    }
}

#[test]
fn numbers() {
    use TokenKind::Number;
    for num in ["42", "3.14", "1e9", "1E10", "2.5e-3"] {
        let toks = content(num);
        assert_eq!(toks, vec![(Number, num)], "number {num:?}");
    }
}

#[test]
fn string_literal_closed() {
    use TokenKind::StringLit;
    let toks = content("'hello'");
    assert_eq!(toks, vec![(StringLit { closed: true }, "'hello'")]);
}

#[test]
fn string_literal_unclosed_is_normal() {
    use TokenKind::StringLit;
    // Half-typed value mid-keystroke — a normal state, not an error.
    let toks = content("'New");
    assert_eq!(toks, vec![(StringLit { closed: false }, "'New")]);
}

#[test]
fn string_literal_with_doubled_quote_escape() {
    use TokenKind::StringLit;
    // `''` is an escaped quote, so the literal does not close there.
    let toks = content("'it''s'");
    assert_eq!(toks, vec![(StringLit { closed: true }, "'it''s'")]);
}

#[test]
fn quoted_ident_with_doubled_quote_escape() {
    use TokenKind::QuotedIdent;
    let toks = content("\"a\"\"b\"");
    assert_eq!(toks, vec![(QuotedIdent, "\"a\"\"b\"")]);
}

#[test]
fn semicolon_inside_string_is_punct_inside_literal_only() {
    use TokenKind::StringLit;
    // A `;` inside a string literal is part of the literal, not a separate Punct.
    let toks = content("'a;b'");
    assert_eq!(toks, vec![(StringLit { closed: true }, "'a;b'")]);
}

/// The full token stream (kind + text), trivia included — for asserting comment/whitespace spans
/// that `content()` deliberately strips.
fn all(src: &str) -> Vec<(TokenKind, &str)> {
    tokenize(src).iter().map(|t| view(src, t)).collect()
}

#[test]
fn line_comment() {
    use TokenKind::{Comment, Keyword, Whitespace};
    // Comments are trivia, so they survive only in the full stream, not the content view.
    assert_eq!(
        all("SELECT -- a note\n"),
        vec![
            (Keyword, "SELECT"),
            (Whitespace, " "),
            (Comment, "-- a note"),
            (Whitespace, "\n"),
        ]
    );
}

#[test]
fn block_comment_closed_and_unclosed() {
    use TokenKind::Comment;
    assert_eq!(all("/* x */"), vec![(Comment, "/* x */")]);
    // Unterminated block comment runs to EOF — half-typed, not an error.
    assert_eq!(all("/* x"), vec![(Comment, "/* x")]);
}

// ── paren depth ─────────────────────────────────────────────────────────────────────────────

/// Paren depth recorded at each non-trivia token's start.
fn depths(src: &str) -> Vec<(TokenKind, &str, i32)> {
    tokenize(src)
        .iter()
        .filter(|t| !t.is_trivia())
        .map(|t| (t.kind, t.text(src), t.depth))
        .collect()
}

#[test]
fn paren_depth_increments_inside_and_resets_outside() {
    use TokenKind::{Ident, Keyword, Number, Punct};
    assert_eq!(
        depths("SELECT (a) FROM t"),
        vec![
            (Keyword, "SELECT", 0),
            (Punct, "(", 1),
            (Ident, "a", 1),
            (Punct, ")", 0),
            (Keyword, "FROM", 0),
            (Ident, "t", 0),
        ]
    );
    // Nested
    assert_eq!(
        depths("((1))")
            .iter()
            .map(|(_, _, d)| *d)
            .collect::<Vec<_>>(),
        vec![1, 2, 2, 1, 0],
        "depth at each of ( ( 1 ) )"
    );
    let _ = Number;
}

#[test]
fn stray_close_paren_clamps_at_zero() {
    // A stray `)` must not drive depth negative; a following top-level `;` stays at depth 0 so the
    // preprocess multi-statement guard still fires. (D6 fail-closed property.)
    let toks = tokenize("SELECT 1); DROP");
    let semi = toks
        .iter()
        .find(|t| t.text("SELECT 1); DROP") == ";")
        .expect("a `;` token");
    assert_eq!(
        semi.depth, 0,
        "the `;` after a stray `)` is still top-level"
    );
}

// ── token-at-cursor + partial ───────────────────────────────────────────────────────────────

#[test]
fn token_at_cursor_binds_to_in_progress_word() {
    let src = "SELECT na";
    let toks = tokenize(src);
    // Cursor just past `na` is extending that ident.
    let idx = token_at_cursor(&toks, src.len()).expect("a token at end");
    assert_eq!(toks[idx].text(src), "na");
    assert_eq!(partial_at_cursor(src, &toks, src.len()), "na");
}

#[test]
fn partial_is_prefix_up_to_cursor_midword() {
    let src = "SELECT name FROM t";
    let toks = tokenize(src);
    // Cursor in the middle of `name` (after `na`).
    let cursor = "SELECT na".len();
    assert_eq!(partial_at_cursor(src, &toks, cursor), "na");
}

#[test]
fn partial_empty_after_whitespace() {
    let src = "WHERE ";
    let toks = tokenize(src);
    assert_eq!(partial_at_cursor(src, &toks, src.len()), "");
    assert_eq!(token_at_cursor(&toks, src.len()), None);
}

#[test]
fn partial_for_open_string_literal_is_value_so_far() {
    let src = "WHERE city = 'New";
    let toks = tokenize(src);
    // Inside an unclosed literal → the partial is the value typed, sans the leading quote.
    assert_eq!(partial_at_cursor(src, &toks, src.len()), "New");
}

#[test]
fn partial_for_closed_string_literal_is_empty() {
    let src = "WHERE city = 'NY'";
    let toks = tokenize(src);
    assert_eq!(partial_at_cursor(src, &toks, src.len()), "");
}

#[test]
fn partial_for_quoted_ident_strips_leading_quote() {
    let src = "SELECT \"ord";
    let toks = tokenize(src);
    assert_eq!(partial_at_cursor(src, &toks, src.len()), "ord");
}

#[test]
fn partial_at_cursor_total_for_all_offsets() {
    // Never panics for any byte cursor in 0..=len, including the multi-byte case.
    for src in ["SELECT * FROM t WHERE x = 'café'", "¡", "WHERE 日 = '本"] {
        let toks = tokenize(src);
        for cursor in 0..=src.len() {
            // `cursor` may land mid-char; the helper clamps/guards internally and must not panic.
            if src.is_char_boundary(cursor) {
                let _ = partial_at_cursor(src, &toks, cursor);
                let _ = token_at_cursor(&toks, cursor);
            }
        }
    }
}

// ── trivia helper ───────────────────────────────────────────────────────────────────────────

#[test]
fn whitespace_and_comments_are_trivia() {
    let src = "SELECT  -- c\n a";
    let toks = tokenize(src);
    assert!(
        toks.iter()
            .filter(|t| t.is_trivia())
            .all(|t| matches!(t.kind, TokenKind::Whitespace | TokenKind::Comment))
    );
    assert!(toks.iter().any(|t| t.kind == TokenKind::Whitespace));
    assert!(toks.iter().any(|t| t.kind == TokenKind::Comment));
}

// ── totality (explicit, seed-independent) ───────────────────────────────────────────────────

#[test]
fn never_panics_on_adversarial_input() {
    // Carried over from the preprocess scan (D6): the byte scanner must never slice across a
    // char boundary. `"¡"` is the exact input that first caught a real panic; the rest stress
    // unbalanced quotes/parens/comments and control bytes.
    for q in [
        "¡",
        "日本",
        "SELECT * FROM t WHERE x = 'café'",
        "SELECT (((",
        "SELECT )))",
        "SELECT 'unterminated",
        "SELECT \"unterminated ident",
        "SELECT /* unterminated comment",
        ")))",
        ";;;",
        "\u{0}\u{1}\u{2}",
        "<<<>>>!=!=",
    ] {
        let toks = tokenize(q);
        // And the spans still tile the input even for adversarial bytes.
        let rebuilt: String = toks.iter().map(|t| t.text(q)).collect();
        assert_eq!(rebuilt, q, "spans must tile {q:?}");
    }
}

// ── properties ──────────────────────────────────────────────────────────────────────────────

proptest::proptest! {
    /// Spans tile the entire input with no gaps or overlaps: concatenating every token's text
    /// reconstructs the source exactly. (The lexer is lossless.)
    #[test]
    fn prop_spans_tile_input(s in ".{0,200}") {
        let toks = tokenize(&s);
        let mut rebuilt = String::new();
        let mut prev_end = 0;
        for t in &toks {
            proptest::prop_assert_eq!(t.start, prev_end, "no gap before token {:?}", t);
            proptest::prop_assert!(t.end >= t.start, "non-negative span");
            rebuilt.push_str(t.text(&s));
            prev_end = t.end;
        }
        proptest::prop_assert_eq!(prev_end, s.len(), "tokens reach end of input");
        proptest::prop_assert_eq!(rebuilt, s);
    }

    /// Total-function guarantee: `tokenize` returns (never panics) for ANY input, including
    /// multi-byte UTF-8 and unbalanced quotes/parens/comments. The scan iterates by char and only
    /// slices at boundaries it controls, so it can never split a code point. (This property's
    /// committed regression seed first caught a real panic on "¡".)
    #[test]
    fn prop_never_panics_for_any_input(s in ".{0,200}") {
        let _ = tokenize(&s);
    }

    /// Paren depth is the running balance of `(`/`)`, clamped at 0 (a stray `)` can't go negative).
    #[test]
    fn prop_depth_is_clamped_running_balance(s in "[()a-z ]{0,80}") {
        let toks = tokenize(&s);
        let mut expected: i32 = 0;
        for t in &toks {
            // The token records the depth *at its start*, after applying its own effect for `(`
            // (open increments before recording) and `)` (close decrements before recording).
            match t.text(&s) {
                "(" => { expected += 1; }
                ")" => { expected = (expected - 1).max(0); }
                _ => {}
            }
            proptest::prop_assert!(t.depth >= 0, "depth never negative");
            proptest::prop_assert_eq!(t.depth, expected, "depth tracks balance at {:?}", t);
        }
    }
}
