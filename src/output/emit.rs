//! `render_output` — the pure result-set serializer for ciq's four output formats.
//!
//! `dev/PLAN.md` §6.7. One entry point, [`render_output`], dispatches on [`OutputFormat`] to a
//! per-format writer. All four are pure functions of a columnar [`Table`] and its [`Schema`]:
//!
//! - **CSV** — RFC 4180: a field is quoted iff it contains `,`, `"`, CR, or LF; embedded `"` is
//!   doubled. [`Cell::Null`] emits an *empty unquoted* field (distinct from `Text("")`, which is
//!   the empty string too at the CSV layer — CSV cannot represent the null/empty distinction, so
//!   both are empty; the distinction is preserved in JSON, where it is representable).
//! - **TSV** — tab-separated; since TSV has no quoting, a literal tab/CR/LF inside a field is
//!   backslash-escaped (`\t`/`\r`/`\n`) and a literal backslash is doubled, so each record stays
//!   exactly one line with the right field count. `Null` emits an empty field.
//! - **JSON** — an array of objects keyed by column name, with full type fidelity: `Null` ->
//!   `null`, `Int`/`Float` -> JSON number, `Bool` -> `true`/`false`, `Text` -> a JSON string
//!   (so `Text("")` is `""`, the §8/Q12 null-vs-empty distinction JSON *can* represent).
//! - **Markdown** — a GitHub-style table: header row, an alignment separator row (`---:` for
//!   numeric/temporal columns per [`ColumnType::is_right_aligned`], `---` otherwise), then one
//!   row per record; `|` inside a cell is escaped as `\|`. `Null` emits an empty cell.
//!
//! The row order is exactly the engine's row order (the query's `ORDER BY`, or its natural
//! order) — `render_output` never reorders, so the output is deterministic given a deterministic
//! query (the determinism rule).

use crate::engine::types::{Cell, Table};
use crate::schema::Schema;

/// The four export formats (`dev/PLAN.md` §6.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// RFC 4180 comma-separated values.
    Csv,
    /// Tab-separated values (backslash-escaped tabs/newlines).
    Tsv,
    /// JSON array of objects, one per row, with numeric/bool/null type fidelity.
    Json,
    /// GitHub-flavored Markdown table with per-column alignment.
    Markdown,
}

impl OutputFormat {
    /// Parse a `--output` value (case-insensitive). `None` for an unknown name.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "csv" => Some(OutputFormat::Csv),
            "tsv" => Some(OutputFormat::Tsv),
            "json" => Some(OutputFormat::Json),
            "markdown" | "md" => Some(OutputFormat::Markdown),
            _ => None,
        }
    }
}

/// Serialize `rows` to a `String` in `format`. Column names/types/order come from `schema`
/// (which matches `rows`' columns — both derive from the same query result).
///
/// Pure: no I/O, no clock, no engine. The clipboard path passes the returned string to
/// [`crate::clipboard::osc52::copy`]; the `--output` CLI path writes it to stdout.
pub fn render_output(rows: &Table, schema: &Schema, format: OutputFormat) -> String {
    match format {
        OutputFormat::Csv => to_csv(rows, schema),
        OutputFormat::Tsv => to_tsv(rows, schema),
        OutputFormat::Json => to_json(rows, schema),
        OutputFormat::Markdown => to_markdown(rows, schema),
    }
}

/// Column names in `schema` order. The table and schema share column order, so this drives both
/// the header and the per-cell iteration.
fn header_names(schema: &Schema) -> Vec<&str> {
    schema.names().collect()
}

// ---- CSV (RFC 4180) -------------------------------------------------------------------------

/// Whether an RFC-4180 field must be quoted: it contains a comma, a double-quote, CR, or LF.
fn csv_needs_quoting(s: &str) -> bool {
    s.contains([',', '"', '\r', '\n'])
}

/// Quote-and-escape one CSV field per RFC 4180 when needed; otherwise return it as-is.
fn csv_field(s: &str) -> String {
    if csv_needs_quoting(s) {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for ch in s.chars() {
            if ch == '"' {
                out.push('"');
            }
            out.push(ch);
        }
        out.push('"');
        out
    } else {
        s.to_string()
    }
}

fn to_csv(rows: &Table, schema: &Schema) -> String {
    delimited(rows, schema, ',', csv_field)
}

// ---- TSV ------------------------------------------------------------------------------------

/// Escape one TSV field: TSV has no quoting, so a tab/CR/LF (and a literal backslash) would break
/// the one-record-per-line / fixed-field-count contract. Backslash-escape them.
fn tsv_field(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
    out
}

fn to_tsv(rows: &Table, schema: &Schema) -> String {
    delimited(rows, schema, '\t', tsv_field)
}

/// Shared CSV/TSV writer: header line, then one line per row, fields joined by `delim` after
/// `escape`-ing each. `Cell::Null` becomes an empty field. Lines end with `\n` (including the
/// last), matching the line-per-record convention.
fn delimited(rows: &Table, schema: &Schema, delim: char, escape: fn(&str) -> String) -> String {
    let names = header_names(schema);
    let mut out = String::new();

    let header: Vec<String> = names.iter().map(|n| escape(n)).collect();
    out.push_str(&header.join(&delim.to_string()));
    out.push('\n');

    for r in 0..rows.row_count() {
        let row = rows.row(r).unwrap_or_default();
        let fields: Vec<String> = row
            .iter()
            .map(|cell| match cell {
                Cell::Null => String::new(),
                other => escape(&other.display()),
            })
            .collect();
        out.push_str(&fields.join(&delim.to_string()));
        out.push('\n');
    }
    out
}

// ---- JSON -----------------------------------------------------------------------------------

/// Escape a string for a JSON double-quoted literal (RFC 8259): `"`, `\`, the C0 control chars
/// with short escapes, and a `\u00XX` fallback for the rest.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// One JSON value for a cell, with type fidelity. A non-finite float (NaN/Inf has no JSON form)
/// degrades to `null`, the only representable choice.
fn json_value(cell: &Cell) -> String {
    match cell {
        Cell::Null => "null".to_string(),
        Cell::Int(i) => i.to_string(),
        Cell::Float(f) => {
            if f.is_finite() {
                // Round-trippable, no thousands separators; matches `f64::to_string`.
                f.to_string()
            } else {
                "null".to_string()
            }
        }
        Cell::Bool(b) => b.to_string(),
        Cell::Text(s) => json_escape(s),
    }
}

fn to_json(rows: &Table, schema: &Schema) -> String {
    let names = header_names(schema);

    if rows.row_count() == 0 {
        return "[]".to_string();
    }

    let mut out = String::from("[\n");
    for r in 0..rows.row_count() {
        let row = rows.row(r).unwrap_or_default();
        out.push_str("  {");
        let pairs: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let val = row
                    .get(i)
                    .map(|c| json_value(c))
                    .unwrap_or_else(|| "null".into());
                format!("{}: {}", json_escape(name), val)
            })
            .collect();
        out.push_str(&pairs.join(", "));
        out.push('}');
        if r + 1 < rows.row_count() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push(']');
    out
}

// ---- Markdown -------------------------------------------------------------------------------

/// Escape a Markdown table cell: a literal `|` would start a new cell, so escape it; collapse
/// embedded newlines to a space so the cell stays on its row.
fn md_cell(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '|' => out.push_str("\\|"),
            '\n' | '\r' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

fn to_markdown(rows: &Table, schema: &Schema) -> String {
    let names = header_names(schema);
    let cols = schema.columns();

    let mut out = String::new();

    // Header row.
    let header: Vec<String> = names.iter().map(|n| md_cell(n)).collect();
    out.push_str("| ");
    out.push_str(&header.join(" | "));
    out.push_str(" |\n");

    // Alignment separator row: right-aligned (`---:`) for numeric/temporal columns.
    let seps: Vec<&str> = cols
        .iter()
        .map(|c| {
            if c.ty.is_right_aligned() {
                "---:"
            } else {
                "---"
            }
        })
        .collect();
    out.push_str("| ");
    out.push_str(&seps.join(" | "));
    out.push_str(" |\n");

    // Body rows.
    for r in 0..rows.row_count() {
        let row = rows.row(r).unwrap_or_default();
        let fields: Vec<String> = row
            .iter()
            .map(|cell| match cell {
                Cell::Null => String::new(),
                other => md_cell(&other.display()),
            })
            .collect();
        out.push_str("| ");
        out.push_str(&fields.join(" | "));
        out.push_str(" |\n");
    }
    out
}

#[cfg(test)]
#[path = "emit_tests.rs"]
mod emit_tests;
