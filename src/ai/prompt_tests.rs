//! Tests for the shared prompt pieces — the schema grounding (table name + every column + its
//! `ColumnType` badge) and the code-fence unwrapping.

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
fn schema_section_embeds_the_table_name() {
    let p = schema_section(&schema());
    assert!(p.contains("`t`"), "table name present:\n{p}");
}

#[test]
fn schema_section_embeds_every_column_with_its_badge() {
    let p = schema_section(&schema());
    for line in [
        "- id (int)",
        "- status (txt)",
        "- amount (num)",
        "- created_at (date)",
    ] {
        assert!(p.contains(line), "missing {line:?}:\n{p}");
    }
}

#[test]
fn schema_section_follows_schema_order() {
    let p = schema_section(&schema());
    let id = p.find("- id (").unwrap();
    let status = p.find("- status (").unwrap();
    let amount = p.find("- amount (").unwrap();
    let created = p.find("- created_at (").unwrap();
    assert!(id < status && status < amount && amount < created, "{p}");
}

#[test]
fn schema_section_is_deterministic() {
    assert_eq!(schema_section(&schema()), schema_section(&schema()));
}

#[test]
fn empty_schema_is_handled() {
    let p = schema_section(&Schema::default());
    assert!(p.contains("(no columns)"), "empty schema noted:\n{p}");
    assert!(p.contains("`t`"), "table still named:\n{p}");
}

// --- code-fence unwrapping (a model commonly wraps SQL in a fence despite rule 2) ---

#[test]
fn strip_code_fences_unwraps_a_tagged_fence() {
    let reply = "```sql\nSELECT * FROM t WHERE region = 'EU'\n```";
    assert_eq!(
        strip_code_fences(reply),
        "SELECT * FROM t WHERE region = 'EU'"
    );
}

#[test]
fn strip_code_fences_unwraps_an_untagged_fence() {
    let reply = "```\nSELECT 1\n```";
    assert_eq!(strip_code_fences(reply), "SELECT 1");
}

#[test]
fn strip_code_fences_handles_surrounding_whitespace() {
    let reply = "\n  ```sql\nSELECT 1\n```  \n";
    assert_eq!(strip_code_fences(reply), "SELECT 1");
}

#[test]
fn strip_code_fences_leaves_unfenced_sql_trimmed() {
    assert_eq!(strip_code_fences("  SELECT 1  "), "SELECT 1");
    // A backtick mid-query (not an outer fence) is untouched.
    assert_eq!(
        strip_code_fences("SELECT \"a`b\" FROM t"),
        "SELECT \"a`b\" FROM t"
    );
}

#[test]
fn strip_code_fences_unterminated_fence_keeps_the_body() {
    // Defensive: a fence opened but never closed still yields the inner SQL, not the backticks.
    assert_eq!(strip_code_fences("```sql\nSELECT 1"), "SELECT 1");
}
