//! Tests for `render_console` — the ANSI-styled aligned table the `Ctrl+O` exit path prints.
//!
//! Pure `&Table -> String`: the escape codes are just bytes, so the whole output goldens like the
//! other four serializers. We assert on the structural facts (header labels, alignment, NULL
//! treatment, the row-count footer) plus a full `insta` snapshot of the byte string.

use super::render_console;
use crate::engine::types::{Cell, Column, Table};
use crate::schema::ColumnType;

fn strip_ansi(s: &str) -> String {
    // Drop CSI sequences (ESC [ ... m) so structural assertions read the plain text.
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for d in chars.by_ref() {
                if d == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn two_col() -> Table {
    Table::new(vec![
        Column::new("id", ColumnType::Int, vec![Cell::Int(1), Cell::Int(20)]),
        Column::new(
            "region",
            ColumnType::Text,
            vec![Cell::Text("EU".into()), Cell::Null],
        ),
    ])
}

#[test]
fn empty_table_renders_empty() {
    let t = Table::new(vec![]);
    assert_eq!(render_console(&t), "");
}

#[test]
fn header_carries_type_badges() {
    let plain = strip_ansi(&render_console(&two_col()));
    let header = plain.lines().next().unwrap();
    assert!(header.contains("id (int)"), "header: {header}");
    assert!(header.contains("region (txt)"), "header: {header}");
}

#[test]
fn numeric_column_is_right_aligned() {
    let plain = strip_ansi(&render_console(&two_col()));
    // The `id` column header `id (int)` is 8 chars wide; `1` right-aligns under it.
    let rows: Vec<&str> = plain.lines().collect();
    // line 0 header, line 1 rule, line 2 first data row.
    let first = rows[2];
    assert!(first.starts_with("       1"), "row: {first:?}");
}

#[test]
fn null_renders_as_glyph_text() {
    let plain = strip_ansi(&render_console(&two_col()));
    assert!(plain.contains("NULL"), "expected NULL glyph:\n{plain}");
}

#[test]
fn footer_counts_rows() {
    let plain = strip_ansi(&render_console(&two_col()));
    assert!(plain.contains("(2 rows)"), "plain:\n{plain}");
}

#[test]
fn footer_singular_for_one_row() {
    let t = Table::new(vec![Column::new("id", ColumnType::Int, vec![Cell::Int(1)])]);
    let plain = strip_ansi(&render_console(&t));
    assert!(plain.contains("(1 row)"), "plain:\n{plain}");
}

#[test]
fn each_column_uses_its_pastel_hue() {
    // The header + body carry each column's pastel hue (matching the TUI grid). Assert the raw
    // ANSI carries column 0's blue and column 1's green on the header and a data cell.
    let out = render_console(&two_col());
    let blue = crate::theme::grid::header_column(0).fg.unwrap();
    let green = crate::theme::grid::header_column(1).fg.unwrap();
    let (br, bg, bb) = match blue {
        ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
        _ => unreachable!(),
    };
    let (gr, gg, gb) = match green {
        ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
        _ => unreachable!(),
    };
    assert!(
        out.contains(&format!("\x1b[38;2;{br};{bg};{bb}m")),
        "column 0's hue appears"
    );
    assert!(
        out.contains(&format!("\x1b[38;2;{gr};{gg};{gb}m")),
        "column 1's hue appears"
    );
}

#[test]
fn snapshot_console_two_col() {
    insta::assert_snapshot!(render_console(&two_col()));
}
