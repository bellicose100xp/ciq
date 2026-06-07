//! Tests for the pure NL->SQL prompt builder — the schema grounding (table name + every column +
//! its `ColumnType` badge) and the strict single-read-only-SELECT instruction must be present.

use super::*;
use crate::schema::{ColumnMeta, ColumnType, Schema};

fn schema() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("status", ColumnType::Text),
        ColumnMeta::new("amount", ColumnType::Float),
        ColumnMeta::new("created_at", ColumnType::Date),
    ])
}

#[test]
fn prompt_embeds_the_table_name() {
    let p = build_prompt("rows in europe", &schema());
    assert!(p.contains("`t`"), "prompt names the table `t`:\n{p}");
}

#[test]
fn prompt_embeds_every_column_name() {
    let p = build_prompt("rows in europe", &schema());
    for col in ["id", "status", "amount", "created_at"] {
        assert!(p.contains(col), "prompt embeds column {col}:\n{p}");
    }
}

#[test]
fn prompt_embeds_each_column_type_badge() {
    let p = build_prompt("anything", &schema());
    // Each column line is `- name (badge)`; assert the exact name+badge pairing per ColumnType.
    assert!(p.contains("- id (int)"), "{p}");
    assert!(p.contains("- status (txt)"), "{p}");
    assert!(p.contains("- amount (num)"), "{p}");
    assert!(p.contains("- created_at (date)"), "{p}");
}

#[test]
fn prompt_instructs_single_read_only_select() {
    let p = build_prompt("anything", &schema());
    assert!(
        p.contains("ONE read-only DuckDB SQL SELECT"),
        "prompt demands one read-only SELECT:\n{p}"
    );
    // And explicitly forbids DML so a compliant model never tries to mutate.
    assert!(
        p.contains("INSERT") && p.contains("DELETE") && p.contains("DROP"),
        "{p}"
    );
}

#[test]
fn prompt_includes_the_request() {
    let p = build_prompt("count rows by status", &schema());
    assert!(
        p.contains("count rows by status"),
        "prompt carries the NL request:\n{p}"
    );
}

#[test]
fn prompt_is_deterministic() {
    let a = build_prompt("same request", &schema());
    let b = build_prompt("same request", &schema());
    assert_eq!(a, b, "same inputs -> byte-identical prompt");
}

#[test]
fn prompt_column_order_follows_schema_order() {
    let p = build_prompt("x", &schema());
    let id = p.find("- id (").unwrap();
    let status = p.find("- status (").unwrap();
    let amount = p.find("- amount (").unwrap();
    let created = p.find("- created_at (").unwrap();
    assert!(
        id < status && status < amount && amount < created,
        "stable schema order:\n{p}"
    );
}

#[test]
fn empty_schema_is_handled() {
    let p = build_prompt("anything", &Schema::default());
    assert!(p.contains("(no columns)"), "empty schema noted:\n{p}");
    assert!(p.contains("`t`"), "table still named:\n{p}");
}

// --- repair prompt ---

#[test]
fn repair_prompt_embeds_failed_sql_and_error() {
    let p = build_repair_prompt(
        "rows in europe",
        "SELECT * FROM t WHERE regon = 'EU'",
        "Referenced column \"regon\" not found",
        &schema(),
    );
    assert!(p.contains("SELECT * FROM t WHERE regon = 'EU'"), "{p}");
    assert!(p.contains("Referenced column \"regon\" not found"), "{p}");
    assert!(
        p.contains("rows in europe"),
        "original request present:\n{p}"
    );
    // Still grounds on the schema + demands a corrected read-only SELECT.
    assert!(p.contains("- status (txt)"), "{p}");
    assert!(p.contains("read-only"), "{p}");
}
