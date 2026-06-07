//! Tests for `output::emit` — byte-exact goldens per format.
//!
//! Each format is asserted byte-for-byte: RFC-4180 CSV quoting/escaping (and a parse round-trip),
//! the §8/Q12 JSON null-vs-`""` distinction with numeric/bool fidelity, TSV escaping, and
//! Markdown alignment. All pure — no engine, no terminal.

use super::*;
use crate::engine::types::{Cell, Column, Table};
use crate::schema::{ColumnMeta, ColumnType, Schema};

/// A small mixed-type table: an int, a text column with quoting hazards, and a float, plus a
/// schema whose types drive Markdown alignment.
fn sample() -> (Table, Schema) {
    let id = Column::new(
        "id",
        ColumnType::Int,
        vec![Cell::Int(1), Cell::Int(2), Cell::Int(3)],
    );
    let name = Column::new(
        "name",
        ColumnType::Text,
        vec![
            Cell::Text("Ada".into()),
            // commas, quotes, and a newline — the three RFC-4180 quoting triggers.
            Cell::Text("Babbage, \"the\"\nfirst".into()),
            Cell::Null,
        ],
    );
    let amount = Column::new(
        "amount",
        ColumnType::Float,
        vec![Cell::Float(10.5), Cell::Null, Cell::Float(3.0)],
    );
    let table = Table::new(vec![id, name, amount]);
    let schema = Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("name", ColumnType::Text),
        ColumnMeta::new("amount", ColumnType::Float),
    ]);
    (table, schema)
}

#[test]
fn csv_rfc4180_quoting_and_null() {
    let (t, s) = sample();
    let out = render_output(&t, &s, OutputFormat::Csv);
    // Row 2's name needs quoting (comma/quote/newline -> wrapped, inner " doubled); both nulls
    // are empty unquoted fields.
    let expected = "id,name,amount\n\
        1,Ada,10.5\n\
        2,\"Babbage, \"\"the\"\"\nfirst\",\n\
        3,,3\n";
    assert_eq!(out, expected);
}

#[test]
fn csv_round_trips_through_a_reader() {
    // Parse the emitted CSV back with a minimal RFC-4180 reader and confirm the field grid is
    // recovered (the embedded comma/quote/newline survive a round-trip).
    let (t, s) = sample();
    let out = render_output(&t, &s, OutputFormat::Csv);
    let records = parse_csv(&out);
    assert_eq!(
        records,
        vec![
            vec!["id".to_string(), "name".into(), "amount".into()],
            vec!["1".into(), "Ada".into(), "10.5".into()],
            vec!["2".into(), "Babbage, \"the\"\nfirst".into(), "".into()],
            vec!["3".into(), "".into(), "3".into()],
        ]
    );
}

#[test]
fn tsv_escapes_tabs_and_newlines() {
    let v = Column::new(
        "v",
        ColumnType::Text,
        vec![Cell::Text("a\tb\nc\\d".into()), Cell::Null],
    );
    let t = Table::new(vec![v]);
    let s = Schema::new(vec![ColumnMeta::new("v", ColumnType::Text)]);
    let out = render_output(&t, &s, OutputFormat::Tsv);
    assert_eq!(out, "v\na\\tb\\nc\\\\d\n\n");
}

#[test]
fn json_null_vs_empty_string_and_type_fidelity() {
    // The §8/Q12 distinction: Cell::Null -> null, Cell::Text("") -> "".
    let id = Column::new("id", ColumnType::Int, vec![Cell::Int(7), Cell::Int(8)]);
    let note = Column::new(
        "note",
        ColumnType::Text,
        vec![Cell::Text(String::new()), Cell::Null],
    );
    let ok = Column::new(
        "ok",
        ColumnType::Bool,
        vec![Cell::Bool(true), Cell::Bool(false)],
    );
    let amt = Column::new("amt", ColumnType::Float, vec![Cell::Float(1.5), Cell::Null]);
    let t = Table::new(vec![id, note, ok, amt]);
    let s = Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("note", ColumnType::Text),
        ColumnMeta::new("ok", ColumnType::Bool),
        ColumnMeta::new("amt", ColumnType::Float),
    ]);
    let out = render_output(&t, &s, OutputFormat::Json);
    let expected = "[\n\
        \x20 {\"id\": 7, \"note\": \"\", \"ok\": true, \"amt\": 1.5},\n\
        \x20 {\"id\": 8, \"note\": null, \"ok\": false, \"amt\": null}\n\
        ]";
    assert_eq!(out, expected);
}

#[test]
fn json_escapes_string_control_chars() {
    let v = Column::new(
        "v",
        ColumnType::Text,
        vec![Cell::Text("a\"b\\c\nd\te".into())],
    );
    let t = Table::new(vec![v]);
    let s = Schema::new(vec![ColumnMeta::new("v", ColumnType::Text)]);
    let out = render_output(&t, &s, OutputFormat::Json);
    assert_eq!(out, "[\n  {\"v\": \"a\\\"b\\\\c\\nd\\te\"}\n]");
}

#[test]
fn json_empty_table_is_empty_array() {
    let t = Table::new(vec![Column::new("id", ColumnType::Int, vec![])]);
    let s = Schema::new(vec![ColumnMeta::new("id", ColumnType::Int)]);
    assert_eq!(render_output(&t, &s, OutputFormat::Json), "[]");
}

#[test]
fn json_non_finite_float_degrades_to_null() {
    let v = Column::new(
        "v",
        ColumnType::Float,
        vec![Cell::Float(f64::NAN), Cell::Float(f64::INFINITY)],
    );
    let t = Table::new(vec![v]);
    let s = Schema::new(vec![ColumnMeta::new("v", ColumnType::Float)]);
    let out = render_output(&t, &s, OutputFormat::Json);
    assert_eq!(out, "[\n  {\"v\": null},\n  {\"v\": null}\n]");
}

#[test]
fn markdown_alignment_and_pipe_escape() {
    let (t, s) = sample();
    let out = render_output(&t, &s, OutputFormat::Markdown);
    // int + float columns right-align (`---:`); text left-aligns (`---`). A pipe in a cell is
    // escaped; the embedded newline collapses to a space.
    let expected = "| id | name | amount |\n\
        | ---: | --- | ---: |\n\
        | 1 | Ada | 10.5 |\n\
        | 2 | Babbage, \"the\" first |  |\n\
        | 3 |  | 3 |\n";
    assert_eq!(out, expected);
}

#[test]
fn markdown_escapes_literal_pipe() {
    let v = Column::new("v", ColumnType::Text, vec![Cell::Text("a|b".into())]);
    let t = Table::new(vec![v]);
    let s = Schema::new(vec![ColumnMeta::new("v", ColumnType::Text)]);
    let out = render_output(&t, &s, OutputFormat::Markdown);
    assert_eq!(out, "| v |\n| --- |\n| a\\|b |\n");
}

#[test]
fn empty_table_csv_is_just_header() {
    let t = Table::new(vec![Column::new("id", ColumnType::Int, vec![])]);
    let s = Schema::new(vec![ColumnMeta::new("id", ColumnType::Int)]);
    assert_eq!(render_output(&t, &s, OutputFormat::Csv), "id\n");
}

#[test]
fn output_format_parse_is_case_insensitive() {
    assert_eq!(OutputFormat::parse("CSV"), Some(OutputFormat::Csv));
    assert_eq!(OutputFormat::parse("tsv"), Some(OutputFormat::Tsv));
    assert_eq!(OutputFormat::parse("Json"), Some(OutputFormat::Json));
    assert_eq!(
        OutputFormat::parse("markdown"),
        Some(OutputFormat::Markdown)
    );
    assert_eq!(OutputFormat::parse("md"), Some(OutputFormat::Markdown));
    assert_eq!(OutputFormat::parse("xml"), None);
}

// ---- minimal RFC-4180 reader (test-only, to prove the CSV emitter round-trips) -------------

/// Parse RFC-4180 CSV into a grid of records. Supports quoted fields with doubled quotes and
/// embedded delimiters/newlines. Sufficient for round-tripping our emitter's output.
fn parse_csv(input: &str) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut chars = input.chars().peekable();
    let mut in_quotes = false;

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
        } else {
            match ch {
                '"' => in_quotes = true,
                ',' => {
                    record.push(std::mem::take(&mut field));
                }
                '\n' => {
                    record.push(std::mem::take(&mut field));
                    records.push(std::mem::take(&mut record));
                }
                _ => field.push(ch),
            }
        }
    }
    // Trailing field/record if the input didn't end in a newline.
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}
