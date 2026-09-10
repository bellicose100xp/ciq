//! Tests for the chat prompt builders: the stable system prompt (grounding + reply contract +
//! preferences), the per-turn question message across every `QueryContext` arm, the generated
//! query-failed turn, and the two pure helpers.

use super::*;
use crate::engine::types::{Cell, Column};
use crate::schema::{ColumnMeta, ColumnType, Schema};

fn schema() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("status", ColumnType::Text),
        ColumnMeta::new("amount", ColumnType::Float),
    ])
}

fn table(rows: usize) -> Table {
    Table::new(vec![
        Column::new(
            "id",
            ColumnType::Int,
            (0..rows).map(|i| Cell::Int(i as i64)).collect(),
        ),
        Column::new(
            "status",
            ColumnType::Text,
            (0..rows)
                .map(|i| {
                    if i % 2 == 1 {
                        Cell::Null
                    } else {
                        Cell::Text(format!("s{i}"))
                    }
                })
                .collect(),
        ),
        Column::new(
            "amount",
            ColumnType::Float,
            (0..rows).map(|i| Cell::Float(i as f64 + 0.5)).collect(),
        ),
    ])
}

fn section_order(p: &str, sections: &[&str]) {
    let mut last = 0;
    for s in sections {
        let at = p
            .find(s)
            .unwrap_or_else(|| panic!("missing section {s:?} in:\n{p}"));
        assert!(at >= last, "section {s:?} out of order in:\n{p}");
        last = at;
    }
}

// --- build_system_prompt: framing + grounding ---

#[test]
fn system_prompt_opens_with_the_role_and_the_conversation_framing() {
    let p = build_system_prompt(&schema(), None);
    assert!(
        p.starts_with(
            "You are a DuckDB SQL assistant helping the user explore a CSV loaded as table `t`.\n\
             You are in a conversation: each message may refine an earlier one, so read the whole \
             transcript before you answer.\n\n## Table\n"
        ),
        "{p}"
    );
}

#[test]
fn system_prompt_names_the_table_and_every_column_with_its_badge() {
    let s = Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("amount", ColumnType::Float),
        ColumnMeta::new("flag", ColumnType::Bool),
        ColumnMeta::new("day", ColumnType::Date),
        ColumnMeta::new("seen_at", ColumnType::Timestamp),
        ColumnMeta::new("status", ColumnType::Text),
        ColumnMeta::new("blob", ColumnType::Other("BLOB".into())),
    ]);
    let p = build_system_prompt(&s, None);
    assert!(p.contains("The single table is named `t`."), "{p}");
    for line in [
        "- id (int)\n",
        "- amount (num)\n",
        "- flag (bool)\n",
        "- day (date)\n",
        "- seen_at (ts)\n",
        "- status (txt)\n",
        "- blob (oth)\n",
    ] {
        assert!(p.contains(line), "missing {line:?} in:\n{p}");
    }
}

#[test]
fn system_prompt_empty_schema_is_handled() {
    let p = build_system_prompt(&Schema::default(), None);
    assert!(p.contains("- (no columns)"), "{p}");
}

// --- build_system_prompt: the reply contract ---

#[test]
fn system_prompt_states_the_single_json_object_contract() {
    let p = build_system_prompt(&schema(), None);
    assert!(p.contains("## Reply format (STRICT)\n"), "{p}");
    assert!(p.contains("EXACTLY ONE JSON object"), "{p}");
    assert!(p.contains("first character must be `{`"), "{p}");
    assert!(p.contains("and the last `}`"), "{p}");
    assert!(
        p.contains("No markdown fences, no prose outside it."),
        "{p}"
    );
}

#[test]
fn system_prompt_spells_out_the_json_schema_with_both_kinds() {
    let p = build_system_prompt(&schema(), None);
    assert!(
        p.contains(
            r#"Schema: {"reply": "one or two short sentences of plain prose", "suggestions": [{"type": "fix|optimize", "sql": "one read-only DuckDB SELECT against t", "details": "one short line"}]}"#
        ),
        "{p}"
    );
}

#[test]
fn system_prompt_makes_reply_mandatory_and_suggestions_optional() {
    let p = build_system_prompt(&schema(), None);
    assert!(
        p.contains(
            "\"reply\" is always present and is what the user reads, so it must answer the \
             question in plain language."
        ),
        "{p}"
    );
    assert!(
        p.contains("\"suggestions\" may be empty when the answer needs no query"),
        "{p}"
    );
    assert!(p.contains("what a column means"), "{p}");
}

#[test]
fn system_prompt_caps_suggestions_at_the_parser_limit() {
    let p = build_system_prompt(&schema(), None);
    assert_eq!(MAX_SUGGESTIONS, 5);
    assert!(p.contains("At most 5 suggestions."), "{p}");
}

#[test]
fn system_prompt_requires_a_fix_for_a_reported_failure_and_defines_optimize() {
    let p = build_system_prompt(&schema(), None);
    assert!(
        p.contains(
            "When the user's message reports a failing query, include at least one \"fix\"."
        ),
        "{p}"
    );
    assert!(
        p.contains("Use \"optimize\" for a faster or cleaner equivalent of the current query."),
        "{p}"
    );
}

#[test]
fn system_prompt_constrains_the_sql_it_asks_for() {
    let p = build_system_prompt(&schema(), None);
    assert!(
        p.contains(
            "Reference only the listed columns, by exact name; quote identifiers that need it \
             with double quotes."
        ),
        "{p}"
    );
    assert!(
        p.contains("SELECT only, single statement, no trailing semicolon."),
        "{p}"
    );
}

#[test]
fn system_prompt_forbids_diffs_and_asks_for_the_full_refined_query() {
    let p = build_system_prompt(&schema(), None);
    assert!(
        p.contains(
            "When refining an earlier query, produce the FULL new query, not a diff or a fragment."
        ),
        "{p}"
    );
}

#[test]
fn system_prompt_carries_no_per_turn_context() {
    let p = build_system_prompt(&schema(), None);
    assert!(!p.contains("## Current query"), "{p}");
    assert!(!p.contains("## Result"), "{p}");
    assert!(!p.contains("## Error\n"), "{p}");
}

#[test]
fn system_prompt_sections_appear_in_layout_order() {
    let p = build_system_prompt(&schema(), Some("prefer short aliases"));
    section_order(
        &p,
        &[
            "You are a DuckDB SQL assistant",
            "## Table\n",
            "## Reply format (STRICT)\n",
            "## Additional user preferences\n",
        ],
    );
}

// --- build_system_prompt: extra instructions ---

#[test]
fn extra_instructions_are_appended_with_the_precedence_note() {
    let p = build_system_prompt(&schema(), Some("  Always alias aggregates.  "));
    assert!(
        p.ends_with(
            "\n## Additional user preferences\nAlways alias aggregates.\n\
             The reply format rules always take precedence.\n"
        ),
        "{p}"
    );
}

#[test]
fn blank_or_missing_extra_instructions_add_nothing() {
    let none = build_system_prompt(&schema(), None);
    let blank = build_system_prompt(&schema(), Some("  \n\t"));
    assert_eq!(none, blank);
    assert!(!none.contains("## Additional user preferences"), "{none}");
    assert!(none.ends_with("not a diff or a fragment.\n"), "{none}");
}

#[test]
fn system_prompt_is_deterministic() {
    let s = schema();
    assert_eq!(
        build_system_prompt(&s, Some("x")),
        build_system_prompt(&s, Some("x"))
    );
}

// --- build_question ---

#[test]
fn question_leads_with_the_trimmed_question_then_the_current_query() {
    let p = build_question(
        "  which statuses are most common?\n",
        "  SELECT * FROM t WHERE status = 'x'  ",
        &QueryContext::None,
        4000,
    );
    assert_eq!(
        p,
        "which statuses are most common?\n\n\
         ## Current query\n```sql\nSELECT * FROM t WHERE status = 'x'\n```\n\n"
    );
}

#[test]
fn question_empty_query_renders_as_placeholder() {
    for sql in ["", "   \n"] {
        let p = build_question("hi", sql, &QueryContext::None, 4000);
        assert!(p.contains("```sql\n(empty)\n```"), "{p}");
    }
}

#[test]
fn question_with_no_text_still_carries_the_context() {
    let p = build_question("   ", "SELECT 1", &QueryContext::Empty, 4000);
    assert_eq!(
        p,
        "\n\n## Current query\n```sql\nSELECT 1\n```\n\n## Result\n0 rows.\n\n"
    );
}

#[test]
fn question_error_arm_embeds_the_trimmed_message() {
    let p = build_question(
        "why does this break?",
        "SELECT nope FROM t",
        &QueryContext::Error("  Binder Error: column \"nope\" not found\n"),
        4000,
    );
    assert!(
        p.ends_with("## Error\nBinder Error: column \"nope\" not found\n\n"),
        "{p}"
    );
    assert!(!p.contains("## Result"), "{p}");
}

#[test]
fn question_error_arm_truncates_a_long_message() {
    let long = "x".repeat(500);
    let p = build_question("why?", "SELECT 1", &QueryContext::Error(&long), 50);
    let expected = format!("## Error\n{}... [truncated]\n\n", "x".repeat(50));
    assert!(p.ends_with(&expected), "{p}");
    assert!(!p.contains(&"x".repeat(51)), "{p}");
}

#[test]
fn question_rows_arm_reports_totals_and_excerpts_the_sample() {
    let t = table(3);
    let p = build_question(
        "summarize this",
        "SELECT * FROM t",
        &QueryContext::Rows {
            sample: &t,
            total_rows: 1234,
        },
        4000,
    );
    assert!(
        p.ends_with(
            "## Result\n1234 rows total; first 3 shown.\n\
             id (int) | status (txt) | amount (num)\n\
             0 | s0 | 0.5\n\
             1 | NULL | 1.5\n\
             2 | s2 | 2.5\n\n"
        ),
        "{p}"
    );
}

#[test]
fn question_rows_arm_caps_shown_at_sample_rows() {
    let t = table(25);
    let p = build_question(
        "summarize",
        "SELECT * FROM t",
        &QueryContext::Rows {
            sample: &t,
            total_rows: 25,
        },
        4000,
    );
    assert!(p.contains("25 rows total; first 10 shown.\n"), "{p}");
    assert!(p.contains("\n9 | NULL | 9.5\n"), "{p}");
    assert!(
        !p.contains("\n10 | s10 | 10.5"),
        "row 11 not excerpted:\n{p}"
    );
}

#[test]
fn question_rows_arm_truncates_a_long_sample() {
    let t = table(10);
    let p = build_question(
        "summarize",
        "SELECT * FROM t",
        &QueryContext::Rows {
            sample: &t,
            total_rows: 10,
        },
        40,
    );
    let expected = format!(
        "## Result\n10 rows total; first 10 shown.\n{}... [truncated]\n\n",
        &render_sample(&t, SAMPLE_ROWS)[..40]
    );
    assert!(p.ends_with(&expected), "{p}");
}

#[test]
fn question_empty_arm_reports_zero_rows() {
    let p = build_question(
        "anything there?",
        "SELECT * FROM t WHERE 1=0",
        &QueryContext::Empty,
        4000,
    );
    assert!(p.ends_with("## Result\n0 rows.\n\n"), "{p}");
}

#[test]
fn question_none_arm_omits_result_and_error() {
    let p = build_question("what is status?", "", &QueryContext::None, 4000);
    assert!(!p.contains("## Result"), "{p}");
    assert!(!p.contains("## Error"), "{p}");
    assert!(p.ends_with("```sql\n(empty)\n```\n\n"), "{p}");
}

#[test]
fn question_sections_appear_in_layout_order() {
    let t = table(2);
    let p = build_question(
        "group by status",
        "SELECT * FROM t",
        &QueryContext::Rows {
            sample: &t,
            total_rows: 2,
        },
        4000,
    );
    section_order(
        &p,
        &["group by status", "## Current query\n", "## Result\n"],
    );
}

#[test]
fn question_is_deterministic() {
    let t = table(4);
    let build = || {
        build_question(
            "q",
            "SELECT * FROM t",
            &QueryContext::Rows {
                sample: &t,
                total_rows: 4,
            },
            100,
        )
    };
    assert_eq!(build(), build());
}

// --- build_failure_turn ---

#[test]
fn failure_turn_reads_as_a_user_message_asking_for_fixes() {
    let p = build_failure_turn("SELECT nope FROM t", "Binder Error: no column", 4000);
    assert_eq!(
        p,
        "The query I just ran failed. Tell me what went wrong and give me fixes.\n\n\
         ## Failing query\n```sql\nSELECT nope FROM t\n```\n\n\
         ## Error\nBinder Error: no column\n"
    );
}

#[test]
fn failure_turn_trims_the_sql_and_the_error() {
    let p = build_failure_turn("  SELECT 1  ", "  boom\n\n", 4000);
    assert!(p.contains("```sql\nSELECT 1\n```"), "{p}");
    assert!(p.ends_with("## Error\nboom\n"), "{p}");
}

#[test]
fn failure_turn_truncates_a_long_error() {
    let long = "y".repeat(300);
    let p = build_failure_turn("SELECT 1", &long, 12);
    assert!(
        p.ends_with(&format!("## Error\n{}... [truncated]\n", "y".repeat(12))),
        "{p}"
    );
    assert!(!p.contains(&"y".repeat(13)), "{p}");
}

#[test]
fn failure_turn_empty_sql_renders_as_placeholder() {
    let p = build_failure_turn("   ", "boom", 4000);
    assert!(p.contains("## Failing query\n```sql\n(empty)\n```"), "{p}");
}

#[test]
fn failure_turn_is_deterministic() {
    let build = || build_failure_turn("SELECT 1", "boom", 100);
    assert_eq!(build(), build());
}

// --- the removed suggestion kind must not resurface ---

#[test]
fn nothing_emits_the_removed_next_kind() {
    let t = table(2);
    let prompts = [
        build_system_prompt(&schema(), Some("prefer CTEs")),
        build_question(
            "q",
            "SELECT 1",
            &QueryContext::Rows {
                sample: &t,
                total_rows: 2,
            },
            4000,
        ),
        build_question("q", "SELECT 1", &QueryContext::Error("boom"), 4000),
        build_failure_turn("SELECT 1", "boom", 4000),
    ];
    for p in prompts {
        assert!(
            !p.to_ascii_lowercase().contains("next"),
            "the `next` kind is gone; it must not appear in:\n{p}"
        );
    }
}

// --- render_sample ---

#[test]
fn render_sample_header_then_rows_with_null_marker() {
    assert_eq!(
        render_sample(&table(2), 10),
        "id (int) | status (txt) | amount (num)\n0 | s0 | 0.5\n1 | NULL | 1.5"
    );
}

#[test]
fn render_sample_zero_rows_is_header_only() {
    assert_eq!(
        render_sample(&table(0), 10),
        "id (int) | status (txt) | amount (num)"
    );
}

#[test]
fn render_sample_respects_max_rows() {
    let out = render_sample(&table(6), 2);
    assert_eq!(out.lines().count(), 3, "header + 2 rows:\n{out}");
    assert!(out.ends_with("1 | NULL | 1.5"), "{out}");
    assert_eq!(render_sample(&table(6), 0), render_sample(&table(0), 0));
}

#[test]
fn render_sample_distinguishes_null_from_empty_text_and_shows_bools() {
    let t = Table::new(vec![
        Column::new(
            "flag",
            ColumnType::Bool,
            vec![Cell::Bool(true), Cell::Bool(false)],
        ),
        Column::new(
            "note",
            ColumnType::Text,
            vec![Cell::Text(String::new()), Cell::Null],
        ),
    ]);
    assert_eq!(
        render_sample(&t, 10),
        "flag (bool) | note (txt)\ntrue | \nfalse | NULL"
    );
}

#[test]
fn render_sample_table_with_no_columns_is_empty_string() {
    assert_eq!(render_sample(&Table::default(), 10), "");
}

// --- truncate_chars ---

#[test]
fn truncate_chars_leaves_short_and_exact_length_input_alone() {
    assert_eq!(truncate_chars("abc", 10), "abc");
    assert_eq!(truncate_chars("abc", 3), "abc");
    assert_eq!(truncate_chars("", 0), "");
}

#[test]
fn truncate_chars_cuts_ascii_with_marker() {
    assert_eq!(truncate_chars("abcdef", 4), "abcd... [truncated]");
    assert_eq!(truncate_chars("abc", 0), "... [truncated]");
}

#[test]
fn truncate_chars_counts_chars_not_bytes() {
    // CJK: 3 bytes per char.
    assert_eq!(truncate_chars("日本語テキスト", 3), "日本語... [truncated]");
    assert_eq!(truncate_chars("日本語", 3), "日本語");
    // Emoji: 4 bytes per char.
    assert_eq!(truncate_chars("🍎🍊🍋🍇", 2), "🍎🍊... [truncated]");
    // Mixed widths cut mid-string still land on a boundary.
    assert_eq!(truncate_chars("a日🍎b", 3), "a日🍎... [truncated]");
    assert_eq!(truncate_chars("a日🍎b", 4), "a日🍎b");
}
