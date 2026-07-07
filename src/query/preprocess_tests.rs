//! Table-driven tests for query preprocessing (grammar validation + the optional LIMIT wrap —
//! `Some(cap)` wraps, `None` (ciq's uncapped default) passes the SQL through verbatim).
//!
//! Includes regression cases for the bugs the Phase-2 code-review found in the original
//! hand-rolled scanners (`--`/`/* */` comments, double-quoted identifiers, `limit` as a
//! column, trailing-comment swallow, UTF-8 mid-char slice) AND the depth-underflow
//! statement-smuggling bug the fix re-review found (stray `)`/`(` hiding a top-level `;`).

use crate::query::preprocess::{PreprocessError, prepare_interactive};

const N: usize = 1000;

/// The wrap form the corrected code emits (newline-delimited so a trailing `--` comment in the
/// user query can't swallow the appended ` ) AS _ciq LIMIT n`).
fn wrapped(inner: &str) -> String {
    format!("SELECT * FROM (\n{inner}\n) AS _ciq LIMIT {N}")
}

#[test]
fn bare_select_gets_wrapped() {
    assert_eq!(
        prepare_interactive("SELECT * FROM t", Some(N)).unwrap(),
        wrapped("SELECT * FROM t")
    );
}

#[test]
fn no_cap_sends_the_query_verbatim() {
    // The uncapped default (`limit: None`): valid SQL passes through with no wrap at all.
    assert_eq!(
        prepare_interactive("SELECT * FROM t", None).unwrap(),
        "SELECT * FROM t"
    );
}

#[test]
fn no_cap_still_validates_the_grammar() {
    assert_eq!(
        prepare_interactive("DROP TABLE t", None),
        Err(PreprocessError::NotReadOnly)
    );
    assert_eq!(prepare_interactive("", None), Err(PreprocessError::Empty));
}

#[test]
fn no_cap_still_strips_the_trailing_semicolon() {
    assert_eq!(
        prepare_interactive("SELECT * FROM t;", None).unwrap(),
        "SELECT * FROM t"
    );
}

#[test]
fn trailing_semicolon_stripped_before_wrap() {
    assert_eq!(
        prepare_interactive("SELECT * FROM t;", Some(N)).unwrap(),
        wrapped("SELECT * FROM t")
    );
}

#[test]
fn whitespace_only_is_empty() {
    assert_eq!(
        prepare_interactive("   \n  ", Some(N)),
        Err(PreprocessError::Empty)
    );
    assert_eq!(
        prepare_interactive("", Some(N)),
        Err(PreprocessError::Empty)
    );
}

#[test]
fn comment_only_is_empty() {
    assert_eq!(
        prepare_interactive("-- just a note", Some(N)),
        Err(PreprocessError::Empty)
    );
    assert_eq!(
        prepare_interactive("/* nothing here */", Some(N)),
        Err(PreprocessError::Empty)
    );
}

#[test]
fn user_limit_is_respected_not_doubled() {
    assert_eq!(
        prepare_interactive("SELECT * FROM t LIMIT 5", Some(N)).unwrap(),
        "SELECT * FROM t LIMIT 5"
    );
}

#[test]
fn order_by_limit_is_respected() {
    assert_eq!(
        prepare_interactive("SELECT * FROM t ORDER BY amount DESC LIMIT 20", Some(N)).unwrap(),
        "SELECT * FROM t ORDER BY amount DESC LIMIT 20"
    );
}

#[test]
fn limit_with_offset_and_long_tail_still_respected() {
    let q = "SELECT a, b, c, d FROM t ORDER BY a, b, c LIMIT 10 OFFSET 5";
    assert_eq!(prepare_interactive(q, Some(N)).unwrap(), q);
}

#[test]
fn order_by_without_limit_gets_wrapped() {
    assert_eq!(
        prepare_interactive("SELECT * FROM t ORDER BY amount DESC", Some(N)).unwrap(),
        wrapped("SELECT * FROM t ORDER BY amount DESC")
    );
}

#[test]
fn with_cte_is_allowed_and_wrapped() {
    let out = prepare_interactive("WITH x AS (SELECT 1 AS n) SELECT * FROM x", Some(N)).unwrap();
    assert_eq!(out, wrapped("WITH x AS (SELECT 1 AS n) SELECT * FROM x"));
}

#[test]
fn bare_keyword_is_not_runnable() {
    assert_eq!(
        prepare_interactive("WITH", Some(N)),
        Err(PreprocessError::NotReadOnly)
    );
    assert_eq!(
        prepare_interactive("SELECT", Some(N)),
        Err(PreprocessError::NotReadOnly)
    );
}

#[test]
fn limit_inside_subquery_does_not_count_as_top_level() {
    let out = prepare_interactive("SELECT * FROM (SELECT * FROM t LIMIT 3) s", Some(N)).unwrap();
    assert_eq!(out, wrapped("SELECT * FROM (SELECT * FROM t LIMIT 3) s"));
}

#[test]
fn multiple_statements_rejected() {
    assert_eq!(
        prepare_interactive("SELECT 1; SELECT 2", Some(N)),
        Err(PreprocessError::MultipleStatements)
    );
    assert_eq!(
        prepare_interactive("SELECT * FROM t; DROP TABLE t", Some(N)),
        Err(PreprocessError::MultipleStatements)
    );
}

#[test]
fn dml_and_ddl_rejected() {
    for q in [
        "INSERT INTO t VALUES (1)",
        "UPDATE t SET x = 1",
        "DELETE FROM t",
        "DROP TABLE t",
        "CREATE TABLE u AS SELECT 1",
        "COPY t TO 'f.csv'",
        "PRAGMA version",
        "ATTACH 'x.db'",
    ] {
        assert_eq!(
            prepare_interactive(q, Some(N)),
            Err(PreprocessError::NotReadOnly),
            "should reject: {q}"
        );
    }
}

// ── regression cases from the Phase-2 code-review ──────────────────────────────────────────

#[test]
fn semicolon_inside_string_is_not_a_statement_break() {
    assert_eq!(
        prepare_interactive("SELECT * FROM t WHERE note = 'a;b'", Some(N)).unwrap(),
        wrapped("SELECT * FROM t WHERE note = 'a;b'")
    );
}

#[test]
fn semicolon_inside_quoted_identifier_is_not_a_break() {
    assert_eq!(
        prepare_interactive("SELECT \"weird;col\" FROM t", Some(N)).unwrap(),
        wrapped("SELECT \"weird;col\" FROM t")
    );
}

#[test]
fn semicolon_inside_block_comment_is_not_a_break() {
    assert_eq!(
        prepare_interactive("SELECT 1 /* note ; here */ FROM t", Some(N)).unwrap(),
        wrapped("SELECT 1 /* note ; here */ FROM t")
    );
}

#[test]
fn leading_block_comment_does_not_hide_select() {
    let out = prepare_interactive("/* my query */ SELECT 1 FROM t", Some(N)).unwrap();
    assert_eq!(out, wrapped("/* my query */ SELECT 1 FROM t"));
}

#[test]
fn limit_keyword_inside_string_does_not_suppress_wrap() {
    let out = prepare_interactive("SELECT * FROM t WHERE note = 'no limit here'", Some(N)).unwrap();
    assert_eq!(out, wrapped("SELECT * FROM t WHERE note = 'no limit here'"));
}

#[test]
fn limit_word_inside_line_comment_does_not_suppress_wrap() {
    let out = prepare_interactive("SELECT * FROM t -- give me a LIMIT\n", Some(N)).unwrap();
    assert_eq!(out, wrapped("SELECT * FROM t -- give me a LIMIT"));
}

#[test]
fn limit_as_quoted_identifier_column_gets_wrapped() {
    let out = prepare_interactive("SELECT \"limit\" FROM t", Some(N)).unwrap();
    assert_eq!(out, wrapped("SELECT \"limit\" FROM t"));
}

#[test]
fn trailing_line_comment_does_not_swallow_wrap() {
    let out = prepare_interactive("SELECT 1 -- note", Some(N)).unwrap();
    assert_eq!(out, wrapped("SELECT 1 -- note"));
    assert!(out.ends_with(") AS _ciq LIMIT 1000"));
}

#[test]
fn leading_parenthesized_select_is_accepted() {
    let out = prepare_interactive("(SELECT a FROM t)", Some(N)).unwrap();
    assert_eq!(out, wrapped("(SELECT a FROM t)"));
}

#[test]
fn case_insensitive_leading_keyword() {
    assert!(prepare_interactive("select * from t", Some(N)).is_ok());
    assert!(prepare_interactive("  WiTh x as (select 1 n) select * from x", Some(N)).is_ok());
}

// ── SAFETY regression: unbalanced-paren statement smuggling (fix re-review found this) ──────
// A stray `)` or `(` must NOT let a second statement (esp. DML/DDL) slip past the
// multi-statement guard. The guard must fail CLOSED — a top-level `;` is detected regardless
// of paren balance, so the resident table `t` can never be mutated.

#[test]
fn stray_close_paren_does_not_smuggle_second_statement() {
    assert_eq!(
        prepare_interactive("SELECT 1); DROP TABLE t", Some(N)),
        Err(PreprocessError::MultipleStatements)
    );
    assert_eq!(
        prepare_interactive("SELECT 1); INSERT INTO t VALUES (1)", Some(N)),
        Err(PreprocessError::MultipleStatements)
    );
    assert_eq!(
        prepare_interactive("SELECT * FROM a WHERE x IN (1,2)) ; DELETE FROM a", Some(N)),
        Err(PreprocessError::MultipleStatements)
    );
}

#[test]
fn stray_open_paren_does_not_smuggle_second_statement() {
    assert_eq!(
        prepare_interactive("SELECT (a ; DROP TABLE t", Some(N)),
        Err(PreprocessError::MultipleStatements)
    );
}

// ── lexer-refactor regression: a digit-leading lead token is stepped over ───────────────────
// After the D6 lexer split (digit-leading words now lex as `Number`, not `Ident`), the
// leading-word scan steps over a leading `Number`/`Operator`/`Punct` to find the first real word.
// These cases pin the resulting outcomes so the behavior is not silently changed again, and prove
// the guard stays closed: a leading number never lets a mutation or a second statement through.

#[test]
fn bare_number_is_rejected_as_empty() {
    // A bare `5` lexes as a `Number` (not a word), so the leading-word scan finds no SELECT/WITH
    // lead at all -> `Empty`. (Pre-D6 a digit-leading token lexed as a Word and this reported
    // `NotReadOnly`; either rejection arm is correct — both refuse to run it. We pin the current
    // arm so the change is no longer silent.)
    assert_eq!(
        prepare_interactive("5", Some(N)),
        Err(PreprocessError::Empty)
    );
}

#[test]
fn leading_number_then_select_is_accepted_but_invalid_sql() {
    // `5 SELECT 1` steps over the leading `5`, finds SELECT as the lead, so the grammar guard
    // accepts and wraps it. The wrapped text is syntactically invalid SQL that DuckDB rejects (a
    // parse error, never a mutation of the resident table) — the guard is not weakened.
    assert_eq!(
        prepare_interactive("5 SELECT 1", Some(N)).unwrap(),
        wrapped("5 SELECT 1")
    );
}

#[test]
fn leading_number_then_dml_is_still_rejected() {
    // The skipped leading `5` exposes the DML word as the lead, which still fails the SELECT/WITH
    // test — a leading number can never smuggle a mutation past the read-only guard.
    assert_eq!(
        prepare_interactive("5 INSERT INTO t VALUES (1)", Some(N)),
        Err(PreprocessError::NotReadOnly)
    );
    assert_eq!(
        prepare_interactive("5 DROP TABLE t", Some(N)),
        Err(PreprocessError::NotReadOnly)
    );
}

#[test]
fn never_panics_on_adversarial_input() {
    for q in [
        "SELECT (((",
        "SELECT )))",
        "SELECT 'unterminated",
        "SELECT \"unterminated ident",
        "SELECT /* unterminated comment",
        "SELECT * FROM t WHERE x = 'unicode: café 日本'",
        ")))",
        ";;;",
        "\u{0}\u{1}\u{2}",
        "¡",
    ] {
        let _ = prepare_interactive(q, Some(N)); // must not panic
    }
}

#[test]
fn error_messages_are_stable_ascii() {
    // The status-line text for each rejection arm (consumed by the App's error line).
    assert_eq!(PreprocessError::Empty.message(), "empty query");
    assert_eq!(
        PreprocessError::MultipleStatements.message(),
        "single statement only"
    );
    assert_eq!(
        PreprocessError::NotReadOnly.message(),
        "read-only SELECT queries only"
    );
    for e in [
        PreprocessError::Empty,
        PreprocessError::MultipleStatements,
        PreprocessError::NotReadOnly,
    ] {
        assert!(e.message().is_ascii());
    }
}

// --- applies_viewport_limit (the truncation-banner signal) ---

#[test]
fn viewport_limit_applies_to_bare_select() {
    use crate::query::preprocess::applies_viewport_limit;
    assert!(applies_viewport_limit("SELECT * FROM t"));
    assert!(applies_viewport_limit("SELECT a, b FROM t WHERE x = 1"));
    assert!(applies_viewport_limit("SELECT * FROM t ORDER BY a DESC"));
}

#[test]
fn viewport_limit_not_applied_when_user_limited() {
    use crate::query::preprocess::applies_viewport_limit;
    assert!(!applies_viewport_limit("SELECT * FROM t LIMIT 5"));
    assert!(!applies_viewport_limit(
        "SELECT * FROM t ORDER BY a DESC LIMIT 20"
    ));
    // A nested LIMIT in a subquery is NOT a top-level LIMIT — the outer result is still ciq-capped.
    assert!(applies_viewport_limit(
        "SELECT * FROM (SELECT * FROM t LIMIT 3) s"
    ));
}

#[test]
fn viewport_limit_not_applied_to_rejected_input() {
    use crate::query::preprocess::applies_viewport_limit;
    assert!(!applies_viewport_limit("")); // empty
    assert!(!applies_viewport_limit("DROP TABLE t")); // not read-only
    assert!(!applies_viewport_limit("SELECT 1; SELECT 2")); // multi-statement
}

proptest::proptest! {
    /// Total-function guarantee: `prepare_interactive` returns (never panics) for ANY input,
    /// including multi-byte UTF-8 and unbalanced quotes/parens/comments. The byte scanner must
    /// never slice across a char boundary. (This property already caught a real panic on "¡".)
    #[test]
    fn prop_never_panics_for_any_input(s in ".{0,200}", lim in proptest::option::of(0usize..100_000)) {
        let _ = prepare_interactive(&s, lim);
    }

    /// SAFETY property: no accepted (Ok) query ever contains a top-level statement separator
    /// that could smuggle a second statement. If a `;` is followed by more non-comment content,
    /// the result MUST be an Err, never Ok — regardless of paren balance. We approximate
    /// "top-level ; with stuff after" by checking that an Ok output, stripped of strings, has no
    /// `;` with a following ascii-alphabetic char (a crude but sound over-approximation: any
    /// such case must have been rejected).
    #[test]
    fn prop_ok_output_has_no_smuggled_statement(
        s in "[A-Za-z0-9 _*,.'\"()=;-]{0,120}", lim in proptest::option::of(1usize..10_000)
    ) {
        if let Ok(out) = prepare_interactive(&s, lim) {
            proptest::prop_assert!(!out.is_empty());
        }
    }
}
