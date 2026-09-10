//! Prompt construction for the AI chat: one stable system prompt plus one user message per turn.
//!
//! The split is deliberate. [`build_system_prompt`] carries everything that does not change while a
//! conversation runs — the role, the schema grounding, the reply contract, the user's
//! `[ai] extra_instructions` — so it stays byte-identical across turns and can be sent as a
//! cacheable prefix. The per-turn message ([`build_question`] / [`build_failure_turn`]) carries what
//! *does* change: the question, the query on screen, and what that query did.
//!
//! Two turn shapes exist, both sent with `role: user`:
//! - [`build_question`] — the user typed something.
//! - [`build_failure_turn`] — nobody typed anything; a query failed and ciq asks for fixes on the
//!   user's behalf. This is the only message ciq generates unprompted (a *successful* query never
//!   triggers a request).
//!
//! Pure and deterministic: same input, same string; column and row order follow the table.

use crate::ai::prompt::schema_section;
use crate::ai::suggestion::{MAX_SUGGESTIONS, SuggestionKind};
use crate::engine::duckdb_engine::TABLE;
use crate::engine::types::Table;
use crate::schema::Schema;

/// How many result rows are excerpted into the prompt.
pub const SAMPLE_ROWS: usize = 10;

/// What the last query produced, attached to the user's message so the model answers about the data
/// actually on screen.
pub enum QueryContext<'a> {
    /// The engine rejected the query; the message it returned.
    Error(&'a str),
    /// Rows came back; `total_rows` is the full count, `sample` the table to excerpt from.
    Rows {
        sample: &'a Table,
        total_rows: usize,
    },
    /// The query ran and returned zero rows.
    Empty,
    /// No result yet (e.g. the chat opened before the first query ran).
    None,
}

/// Build the system prompt: role, schema grounding, reply contract, and user preferences.
///
/// Mentions neither the current query nor its result, so it is stable for the whole conversation.
pub fn build_system_prompt(schema: &Schema, extra_instructions: Option<&str>) -> String {
    let mut p = String::new();
    p.push_str(&format!(
        "You are a DuckDB SQL assistant helping the user explore a CSV loaded as table `{TABLE}`.\n\
         You are in a conversation: each message may refine an earlier one, so read the whole \
         transcript before you answer.\n\n"
    ));
    p.push_str(&schema_section(schema));
    p.push_str(&reply_contract());

    if let Some(extra) = extra_instructions.map(str::trim).filter(|s| !s.is_empty()) {
        p.push_str("\n## Additional user preferences\n");
        p.push_str(extra);
        p.push_str("\nThe reply format rules always take precedence.\n");
    }
    p
}

/// Build the user message for a typed question, with the live query + result appended.
pub fn build_question(
    question: &str,
    sql: &str,
    outcome: &QueryContext<'_>,
    max_context_chars: usize,
) -> String {
    let mut p = String::new();
    p.push_str(question.trim());
    p.push_str("\n\n");
    p.push_str(&sql_section("Current query", sql));
    p.push_str(&outcome_section(outcome, max_context_chars));
    p
}

/// Build the user message ciq sends by itself when a query fails: the failing SQL, the engine's
/// error, and a request for fixes.
pub fn build_failure_turn(sql: &str, error: &str, max_context_chars: usize) -> String {
    let mut p = String::new();
    p.push_str("The query I just ran failed. Tell me what went wrong and give me fixes.\n\n");
    p.push_str(&sql_section("Failing query", sql));
    p.push_str("## Error\n");
    p.push_str(&truncate_chars(error.trim(), max_context_chars));
    p.push('\n');
    p
}

/// Header row (`name (type badge)` joined by ` | `) then up to `max_rows` rows of cells via
/// `Cell::display()` (`NULL` for nulls) joined by ` | `.
pub fn render_sample(table: &Table, max_rows: usize) -> String {
    let header = table
        .columns()
        .iter()
        .map(|c| format!("{} ({})", c.name, c.ty.badge()))
        .collect::<Vec<_>>()
        .join(" | ");
    let mut lines = vec![header];
    for i in 0..table.row_count().min(max_rows) {
        let Some(cells) = table.row(i) else { break };
        let line = cells
            .iter()
            .map(|c| {
                if c.is_null() {
                    "NULL".to_string()
                } else {
                    c.display()
                }
            })
            .collect::<Vec<_>>()
            .join(" | ");
        lines.push(line);
    }
    lines.join("\n")
}

/// Char-boundary-safe truncation to at most `max_chars` chars, appending `... [truncated]` when cut.
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        None => s.to_string(),
        Some((cut, _)) => format!("{}... [truncated]", &s[..cut]),
    }
}

fn sql_section(heading: &str, sql: &str) -> String {
    let sql = sql.trim();
    format!(
        "## {heading}\n```sql\n{}\n```\n\n",
        if sql.is_empty() { "(empty)" } else { sql }
    )
}

fn outcome_section(outcome: &QueryContext<'_>, max_context_chars: usize) -> String {
    let mut s = String::new();
    match outcome {
        QueryContext::Error(err) => {
            s.push_str("## Error\n");
            s.push_str(&truncate_chars(err.trim(), max_context_chars));
            s.push_str("\n\n");
        }
        QueryContext::Rows { sample, total_rows } => {
            let shown = sample.row_count().min(SAMPLE_ROWS);
            s.push_str(&format!(
                "## Result\n{total_rows} rows total; first {shown} shown.\n"
            ));
            s.push_str(&truncate_chars(
                &render_sample(sample, SAMPLE_ROWS),
                max_context_chars,
            ));
            s.push_str("\n\n");
        }
        QueryContext::Empty => s.push_str("## Result\n0 rows.\n\n"),
        QueryContext::None => {}
    }
    s
}

/// The reply contract. The wire labels come from [`SuggestionKind`] so the prompt and
/// [`parse_reply`](crate::ai::suggestion::parse_reply) can never disagree on spelling.
fn reply_contract() -> String {
    let kinds = format!(
        "{}|{}",
        SuggestionKind::Fix.label(),
        SuggestionKind::Optimize.label()
    );
    let mut s = String::new();
    s.push_str("## Reply format (STRICT)\n");
    s.push_str(
        "1. Reply with EXACTLY ONE JSON object and nothing else: the first character must be `{` \
         and the last `}`. No markdown fences, no prose outside it.\n",
    );
    s.push_str(&format!(
        "2. Schema: {{\"reply\": \"one or two short sentences of plain prose\", \"suggestions\": \
         [{{\"type\": \"{kinds}\", \"sql\": \"one read-only DuckDB SELECT against {TABLE}\", \
         \"details\": \"one short line\"}}]}}\n"
    ));
    s.push_str(
        "3. \"reply\" is always present and is what the user reads, so it must answer the question \
         in plain language.\n",
    );
    s.push_str(
        "4. \"suggestions\" may be empty when the answer needs no query (for example the user asked \
         what a column means).\n",
    );
    s.push_str(&format!("5. At most {MAX_SUGGESTIONS} suggestions.\n"));
    s.push_str(
        "6. When the user's message reports a failing query, include at least one \"fix\".\n",
    );
    s.push_str("7. Use \"optimize\" for a faster or cleaner equivalent of the current query.\n");
    s.push_str(
        "8. Reference only the listed columns, by exact name; quote identifiers that need it with \
         double quotes.\n",
    );
    s.push_str("9. SELECT only, single statement, no trailing semicolon.\n");
    s.push_str(
        "10. When refining an earlier query, produce the FULL new query, not a diff or a \
         fragment.\n",
    );
    s
}

#[cfg(test)]
#[path = "chat_prompt_tests.rs"]
mod chat_prompt_tests;
