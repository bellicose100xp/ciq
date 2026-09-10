//! Tests for the tolerant reply parser: packaging (fences, prose, braces in strings) is forgiven,
//! content (unknown type, blank sql) is dropped, the prose `reply` is always present, and
//! `keep_valid` re-applies the read-only guard.

use super::*;

fn reply_of(raw: &str) -> ChatReply {
    match parse_reply(raw) {
        ParseOutcome::Parsed(r) => r,
        other => panic!("expected Parsed, got {other:?} for:\n{raw}"),
    }
}

fn parsed(raw: &str) -> Vec<Suggestion> {
    reply_of(raw).suggestions
}

fn sugg(kind: SuggestionKind, sql: &str, details: &str) -> Suggestion {
    Suggestion {
        kind,
        sql: sql.to_string(),
        details: details.to_string(),
    }
}

const CLEAN: &str = r#"{"reply":"here are two options","suggestions":[
  {"type":"fix","sql":"SELECT * FROM t LIMIT 5","details":"fix the typo"},
  {"type":"optimize","sql":"SELECT id FROM t","details":"fewer columns"}
]}"#;

// --- SuggestionKind ---

#[test]
fn kind_labels_round_trip() {
    for k in [SuggestionKind::Fix, SuggestionKind::Optimize] {
        assert_eq!(SuggestionKind::parse(k.label()), Some(k));
    }
    assert_eq!(SuggestionKind::Fix.label(), "fix");
    assert_eq!(SuggestionKind::Optimize.label(), "optimize");
}

#[test]
fn the_retired_next_kind_is_rejected() {
    // Predicting the user's next query was noise they never picked, so the kind is gone and an
    // entry still carrying it is dropped rather than shown as something else.
    assert_eq!(SuggestionKind::parse("next"), None);
    let list = parsed(
        r#"{"reply":"ok","suggestions":[
            {"type":"next","sql":"SELECT count(*) FROM t","details":"count"},
            {"type":"fix","sql":"SELECT * FROM t","details":"all rows"}
        ]}"#,
    );
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].kind, SuggestionKind::Fix);
}

#[test]
fn kind_parse_is_case_insensitive_and_trimmed() {
    assert_eq!(SuggestionKind::parse(" FIX "), Some(SuggestionKind::Fix));
    assert_eq!(
        SuggestionKind::parse("Optimize"),
        Some(SuggestionKind::Optimize)
    );
    assert_eq!(SuggestionKind::parse("\tFiX\n"), Some(SuggestionKind::Fix));
    assert_eq!(SuggestionKind::parse("fixes"), None);
    assert_eq!(SuggestionKind::parse(""), None);
}

// --- packaging tolerance ---

#[test]
fn clean_json_parses_all_entries_in_order() {
    let v = parsed(CLEAN);
    assert_eq!(
        v,
        vec![
            sugg(
                SuggestionKind::Fix,
                "SELECT * FROM t LIMIT 5",
                "fix the typo"
            ),
            sugg(
                SuggestionKind::Optimize,
                "SELECT id FROM t",
                "fewer columns"
            ),
        ]
    );
}

#[test]
fn fenced_json_is_unwrapped() {
    let raw = format!("```json\n{CLEAN}\n```");
    assert_eq!(parsed(&raw), parsed(CLEAN));
}

#[test]
fn prose_wrapped_json_is_located() {
    let raw = format!("Sure! Here are some ideas:\n{CLEAN}\nHope this helps.");
    assert_eq!(parsed(&raw), parsed(CLEAN));
}

#[test]
fn braces_inside_strings_do_not_break_the_scan() {
    let raw = r#"Here you go: {"suggestions":[{"type":"optimize","sql":"SELECT '{' || name || '}' FROM t","details":"braces } inside \" strings {"}]} done"#;
    let v = parsed(raw);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].sql, "SELECT '{' || name || '}' FROM t");
    assert_eq!(v[0].details, "braces } inside \" strings {");
}

#[test]
fn a_leading_non_json_brace_is_skipped() {
    let raw = format!("Note {{this is not json}} then {CLEAN}");
    assert_eq!(parsed(&raw), parsed(CLEAN));
}

#[test]
fn an_object_without_suggestions_key_is_skipped_for_a_later_one() {
    let raw = format!(r#"{{"note":"ignored"}} {CLEAN}"#);
    assert_eq!(parsed(&raw), parsed(CLEAN));
}

// --- content normalization ---

#[test]
fn unknown_type_is_skipped() {
    let raw = r#"{"suggestions":[
      {"type":"rewrite","sql":"SELECT 1 FROM t","details":"x"},
      {"type":"optimize","sql":"SELECT 2 FROM t","details":"y"}]}"#;
    let v = parsed(raw);
    assert_eq!(
        v,
        vec![sugg(SuggestionKind::Optimize, "SELECT 2 FROM t", "y")]
    );
}

#[test]
fn blank_sql_is_skipped() {
    let raw = r#"{"suggestions":[
      {"type":"fix","sql":"   ","details":"blank"},
      {"type":"fix","sql":";","details":"just a semicolon"},
      {"type":"fix","details":"missing"},
      {"type":"optimize","sql":"SELECT 2 FROM t","details":"y"}]}"#;
    let v = parsed(raw);
    assert_eq!(
        v,
        vec![sugg(SuggestionKind::Optimize, "SELECT 2 FROM t", "y")]
    );
}

#[test]
fn malformed_entry_types_are_skipped_not_fatal() {
    let raw = r#"{"suggestions":[
      {"type":"fix","sql":42,"details":"numeric sql"},
      {"type":7,"sql":"SELECT 1 FROM t"},
      "not an object",
      {"type":"optimize","sql":"SELECT 2 FROM t","details":"y"}]}"#;
    let v = parsed(raw);
    assert_eq!(
        v,
        vec![sugg(SuggestionKind::Optimize, "SELECT 2 FROM t", "y")]
    );
}

#[test]
fn sql_is_trimmed_and_one_trailing_semicolon_is_stripped() {
    let raw = r#"{"suggestions":[
      {"type":"optimize","sql":"  SELECT 1 FROM t ;  ","details":"a"},
      {"type":"optimize","sql":"SELECT 2 FROM t;;","details":"b"}]}"#;
    let v = parsed(raw);
    assert_eq!(v[0].sql, "SELECT 1 FROM t");
    assert_eq!(v[1].sql, "SELECT 2 FROM t;", "only ONE trailing ';' goes");
}

#[test]
fn sql_internal_newlines_are_kept() {
    let raw =
        r#"{"suggestions":[{"type":"optimize","sql":"SELECT id\nFROM t\nLIMIT 5","details":"a"}]}"#;
    assert_eq!(parsed(raw)[0].sql, "SELECT id\nFROM t\nLIMIT 5");
}

#[test]
fn details_are_trimmed_and_default_to_empty() {
    let raw = r#"{"suggestions":[
      {"type":"optimize","sql":"SELECT 1 FROM t","details":"  padded  "},
      {"type":"optimize","sql":"SELECT 2 FROM t"},
      {"type":"optimize","sql":"SELECT 3 FROM t","details":null}]}"#;
    let v = parsed(raw);
    assert_eq!(v[0].details, "padded");
    assert_eq!(v[1].details, "");
    assert_eq!(v[2].details, "");
}

#[test]
fn duplicates_by_exact_sql_keep_the_first() {
    let raw = r#"{"suggestions":[
      {"type":"fix","sql":"SELECT 1 FROM t","details":"first"},
      {"type":"optimize","sql":"SELECT 1 FROM t;","details":"same after normalization"},
      {"type":"optimize","sql":"select 1 from t","details":"different case is different sql"}]}"#;
    let v = parsed(raw);
    assert_eq!(
        v,
        vec![
            sugg(SuggestionKind::Fix, "SELECT 1 FROM t", "first"),
            sugg(
                SuggestionKind::Optimize,
                "select 1 from t",
                "different case is different sql"
            ),
        ]
    );
}

#[test]
fn capped_at_max_suggestions_in_order() {
    let entries: Vec<String> = (0..8)
        .map(|i| format!(r#"{{"type":"optimize","sql":"SELECT {i} FROM t","details":"d{i}"}}"#))
        .collect();
    let raw = format!(r#"{{"suggestions":[{}]}}"#, entries.join(","));
    let v = parsed(&raw);
    assert_eq!(v.len(), MAX_SUGGESTIONS);
    let sqls: Vec<&str> = v.iter().map(|s| s.sql.as_str()).collect();
    assert_eq!(
        sqls,
        [
            "SELECT 0 FROM t",
            "SELECT 1 FROM t",
            "SELECT 2 FROM t",
            "SELECT 3 FROM t",
            "SELECT 4 FROM t"
        ]
    );
}

#[test]
fn cap_applies_after_dropping_invalid_entries() {
    // Six entries, one unknown type early: the five valid ones all survive.
    let raw = r#"{"suggestions":[
      {"type":"bogus","sql":"SELECT 0 FROM t"},
      {"type":"optimize","sql":"SELECT 1 FROM t"},
      {"type":"optimize","sql":"SELECT 2 FROM t"},
      {"type":"optimize","sql":"SELECT 3 FROM t"},
      {"type":"optimize","sql":"SELECT 4 FROM t"},
      {"type":"optimize","sql":"SELECT 5 FROM t"}]}"#;
    let v = parsed(raw);
    assert_eq!(v.len(), 5);
    assert_eq!(v[4].sql, "SELECT 5 FROM t");
}

// --- outcomes ---

#[test]
fn empty_array_is_empty() {
    // An empty list is a normal answer (the question may need no query), so it parses.
    assert_eq!(
        reply_of(r#"{"reply":"nothing to run","suggestions":[]}"#)
            .suggestions
            .len(),
        0
    );
    assert!(
        reply_of("```json\n{\"reply\":\"none\",\"suggestions\": []}\n```")
            .suggestions
            .is_empty()
    );
}

#[test]
fn all_entries_dropped_is_empty() {
    let raw =
        r#"{"suggestions":[{"type":"nope","sql":"SELECT 1 FROM t"},{"type":"fix","sql":""}]}"#;
    assert!(reply_of(raw).suggestions.is_empty());
}

#[test]
fn nothing_parseable_is_unparseable() {
    assert_eq!(parse_reply(""), ParseOutcome::Unparseable);
    assert_eq!(
        parse_reply("I cannot help with that."),
        ParseOutcome::Unparseable
    );
    assert_eq!(
        parse_reply(r#"{"note":"no suggestions key"}"#),
        ParseOutcome::Unparseable
    );
    assert_eq!(
        parse_reply(r#"{"suggestions": "not an array"}"#),
        ParseOutcome::Unparseable
    );
    assert_eq!(
        parse_reply(r#"{"suggestions":[{"type":"optimize","sql":"SELECT 1"}"#),
        ParseOutcome::Unparseable,
        "unbalanced braces"
    );
    assert_eq!(parse_reply(r#"["suggestions"]"#), ParseOutcome::Unparseable);
}

#[test]
fn parse_is_deterministic() {
    let raw = format!("prose {CLEAN} more prose");
    assert_eq!(parse_reply(&raw), parse_reply(&raw));
}

// --- keep_valid ---

#[test]
fn keep_valid_drops_everything_the_read_only_guard_rejects() {
    let input = vec![
        sugg(SuggestionKind::Fix, "DROP TABLE t", "destructive"),
        sugg(
            SuggestionKind::Optimize,
            "SELECT 1 FROM t; DROP TABLE t",
            "multi",
        ),
        sugg(
            SuggestionKind::Optimize,
            "DELETE FROM t WHERE id = 1",
            "dml",
        ),
        sugg(SuggestionKind::Optimize, "PRAGMA version", "pragma"),
        sugg(SuggestionKind::Optimize, "SELECT", "bare keyword"),
        sugg(SuggestionKind::Optimize, "SELECT * FROM t LIMIT 5", "ok"),
        sugg(
            SuggestionKind::Optimize,
            "WITH c AS (SELECT id FROM t) SELECT count(*) FROM c",
            "cte ok",
        ),
    ];
    let kept = keep_valid(input);
    assert_eq!(
        kept,
        vec![
            sugg(SuggestionKind::Optimize, "SELECT * FROM t LIMIT 5", "ok"),
            sugg(
                SuggestionKind::Optimize,
                "WITH c AS (SELECT id FROM t) SELECT count(*) FROM c",
                "cte ok",
            ),
        ]
    );
}

#[test]
fn keep_valid_preserves_order_and_handles_empty() {
    assert!(keep_valid(Vec::new()).is_empty());
    let input = vec![
        sugg(SuggestionKind::Optimize, "SELECT 2 FROM t", "b"),
        sugg(SuggestionKind::Optimize, "SELECT 1 FROM t", "a"),
    ];
    let kept = keep_valid(input.clone());
    assert_eq!(kept, input);
}

#[test]
fn keep_valid_accepts_lowercase_and_parenthesized_select() {
    let input = vec![
        sugg(SuggestionKind::Optimize, "select id from t", "lower"),
        sugg(SuggestionKind::Optimize, "(SELECT id FROM t)", "parens"),
    ];
    assert_eq!(keep_valid(input.clone()), input);
}
