//! Prompt construction — PURE `build_prompt(nl, &Schema) -> String` (`dev/PLAN.md` §7 P5.1).
//!
//! Ported from jiq's `ai/prompt.rs` idea (ground the model on the actual data shape), retargeted
//! from JSON to a relational table: instead of jiq's JSON-schema-and-jq-rules prose, ciq embeds
//! the **table name + every column name with its [`ColumnType`]** and instructs the model to emit
//! exactly **one read-only DuckDB `SELECT`**. The grounding is the whole point — the model sees
//! the real columns and their types, so it writes valid SQL against `t` rather than guessing.
//!
//! Pure and deterministic: same `(nl, schema)` -> same string, stable column order (schema order),
//! no clock/rand/network. Golden-tested (the prompt must contain the table name, every column +
//! its `ColumnType` badge, and the single-read-only-SELECT instruction).
//!
//! The output reaches the model, whose reply is treated as a single SQL statement and then validated
//! by the existing [`prepare_interactive`](crate::query::preprocess::prepare_interactive) guard —
//! so even if the model ignores the "read-only SELECT only" rule, a DML/multi-statement reply is
//! rejected before it can touch the table. The prompt rules are there to maximize the chance of a
//! *usable* answer, not as the security boundary (that is preprocess).

use crate::engine::duckdb_engine::TABLE;
use crate::schema::Schema;

/// Build the NL->SQL prompt grounding the model on `schema`.
///
/// Embeds: the resident table name (`t`), one `- name (type)` line per column in schema order
/// (the type is the human-readable [`ColumnType`] badge — `int`/`txt`/`date`/…), the strict
/// output rules (one read-only `SELECT`, no prose, no fences, no DML/DDL), and the user's
/// natural-language request. Pure; no I/O.
pub fn build_prompt(nl_query: &str, schema: &Schema) -> String {
    let mut p = String::new();
    p.push_str(
        "You translate a natural-language request into ONE read-only DuckDB SQL SELECT query.\n\n",
    );
    p.push_str(&schema_section(schema));
    p.push_str(&output_rules());
    p.push_str("## Request\n");
    p.push_str(nl_query.trim());
    p.push('\n');
    p
}

/// Build a repair prompt for an error round-trip: the original request, the SQL the model
/// produced, and the engine's error message, asking for a corrected single read-only `SELECT`.
///
/// Optional convenience for a future "the generated query failed — try again" loop. Pure; same
/// schema grounding + output rules as [`build_prompt`], plus the failed SQL and its error.
pub fn build_repair_prompt(
    nl_query: &str,
    failed_sql: &str,
    error: &str,
    schema: &Schema,
) -> String {
    let mut p = String::new();
    p.push_str(
        "A DuckDB SQL query you generated failed. Produce a CORRECTED read-only SELECT query.\n\n",
    );
    p.push_str(&schema_section(schema));
    p.push_str("## Failed query\n");
    p.push_str(failed_sql.trim());
    p.push_str("\n\n## Error\n");
    p.push_str(error.trim());
    p.push_str("\n\n");
    p.push_str(&output_rules());
    p.push_str("## Original request\n");
    p.push_str(nl_query.trim());
    p.push('\n');
    p
}

/// The schema-grounding section: the table name + one `- name (type)` line per column, in schema
/// order. This is the ciq analog of jiq's JSON-schema block — the model's view of the real data.
fn schema_section(schema: &Schema) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "## Table\nThe single table is named `{TABLE}`. Its columns (name and type):\n"
    ));
    if schema.is_empty() {
        s.push_str("- (no columns)\n");
    } else {
        for col in schema.columns() {
            s.push_str(&format!("- {} ({})\n", col.name, col.ty.badge()));
        }
    }
    s.push('\n');
    s
}

/// The strict output rules — one read-only `SELECT`, nothing else. Mirrors the *intent* of jiq's
/// strict-format block (maximize a parseable answer) without jiq's JSON-suggestions schema: ciq
/// wants exactly one SQL statement back.
fn output_rules() -> String {
    let mut s = String::new();
    s.push_str("## Output rules (STRICT)\n");
    s.push_str(&format!(
        "1. Reply with EXACTLY ONE read-only DuckDB SQL SELECT statement against table `{TABLE}`.\n"
    ));
    s.push_str(
        "2. The reply MUST be only the SQL — no markdown code fences, no prose, no commentary.\n",
    );
    s.push_str(
        "3. Do NOT emit INSERT, UPDATE, DELETE, COPY, CREATE, DROP, ALTER, ATTACH, PRAGMA, or any \
         statement that mutates data — only a single SELECT (a leading WITH ... SELECT CTE is fine).\n",
    );
    s.push_str("4. Do NOT emit multiple statements; no trailing semicolon is needed.\n");
    s.push_str("5. Reference only the columns listed above, by their exact names.\n\n");
    s
}

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod prompt_tests;
