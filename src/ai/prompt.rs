//! Shared prompt pieces — PURE (`dev/PLAN.md` §7 P5.1).
//!
//! [`schema_section`] grounds the model on the actual data shape: the **table name + every column
//! name with its [`ColumnType`](crate::schema::ColumnType)**, in schema order, so it writes valid SQL against `t` rather than
//! guessing. The chat's system prompt ([`chat_prompt`](super::chat_prompt)) embeds it.
//! [`strip_code_fences`] unwraps the one fence a model may add despite being told not to.
//!
//! Pure and deterministic: same input -> same string, no clock/rand/network. The prompt is there to
//! maximize the chance of a *usable* answer, not as the security boundary — every reply still
//! crosses [`prepare_interactive`](crate::query::preprocess::prepare_interactive) before it can
//! touch the table.

use crate::engine::duckdb_engine::TABLE;
use crate::schema::Schema;

/// Strip a single outer markdown code fence (``` or ```sql … ```) and surrounding whitespace from
/// a model reply, returning the inner text. Purely cosmetic unwrapping: the system prompt asks
/// the model for no fences, but that is best-effort, not the boundary — LLMs commonly wrap SQL
/// in a fence anyway, which would otherwise lex with a leading backtick/`sql` token and be rejected
/// by [`prepare_interactive`](crate::query::preprocess::prepare_interactive) as "read-only SELECT
/// queries only". Unwrapping here lets a perfectly good `SELECT` reach the *same* read-only guard
/// (the guard stays the security boundary; this only removes fence noise). A reply with no fence is
/// returned trimmed and otherwise untouched.
pub fn strip_code_fences(reply: &str) -> String {
    let trimmed = reply.trim();
    let Some(after_open) = trimmed.strip_prefix("```") else {
        return trimmed.to_string();
    };
    // Drop the rest of the opening-fence line (an optional info string like `sql`).
    let body_start = after_open.find('\n').map_or(after_open.len(), |i| i + 1);
    let body = &after_open[body_start..];
    // Remove a closing fence if present; otherwise keep the body as-is (an unterminated fence).
    let inner = match body.rfind("```") {
        Some(close) => &body[..close],
        None => body,
    };
    inner.trim().to_string()
}

/// The schema-grounding section: the table name + one `- name (type)` line per column, in schema
/// order. This is the ciq analog of jiq's JSON-schema block — the model's view of the real data.
pub fn schema_section(schema: &Schema) -> String {
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

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod prompt_tests;
