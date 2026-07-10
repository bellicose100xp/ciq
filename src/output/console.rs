//! `render_console` — the ANSI-styled aligned table printed to stdout on a `Ctrl+O` exit.
//!
//! ciq's analog of jiq's output-on-exit deliverable (jiq prints its colored JSON result after
//! the terminal is restored): the user quits with `Ctrl+O` and the result they were looking at
//! lands in the scrollback, aligned and colored like the in-TUI grid — headers in the grid's
//! cyan, `NULL`s dimmed, numeric columns right-aligned, a quiet row-count footer.
//!
//! **Pure** `(&Table) -> String` (the §6.7 discipline): no terminal, no clock, no I/O — the
//! escape codes are just bytes, so the whole output is a snapshot-testable golden. Colors are
//! derived from the same [`theme::base`](crate::theme::base) palette the TUI paints with (the
//! one-source-of-truth theme rule), translated to 24-bit ANSI here at the string edge.

use ratatui::style::Color;

use crate::engine::Table;
use crate::grid::col_width::{cell_display, header_label};
use crate::theme;

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// The 24-bit ANSI foreground sequence for a theme RGB color (empty for non-RGB, which the
/// [`theme::base`](crate::theme::base) palette never produces).
fn ansi_fg(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("\x1b[38;2;{r};{g};{b}m"),
        _ => String::new(),
    }
}

/// The ANSI foreground for the column at index `idx` — the same pastel hue the TUI grid paints
/// (via [`theme::grid::column`]), so a `Ctrl+O` dump matches what was on screen. Falls back to
/// empty for the (never-produced) non-RGB case.
fn column_fg(idx: usize) -> String {
    theme::grid::column(idx).fg.map(ansi_fg).unwrap_or_default()
}

/// The ANSI foreground for the header label of column `idx` — its pastel hue (via
/// [`theme::grid::header_column`]), matching the TUI's per-column headers.
fn header_fg(idx: usize) -> String {
    theme::grid::header_column(idx)
        .fg
        .map(ansi_fg)
        .unwrap_or_default()
}

/// Serialize `rows` as an ANSI-colored aligned table for the console: a bold header row (the
/// grid's `name (type)` labels), a dim rule under it, one line per row with the grid's alignment
/// and NULL treatment, and a dim `(N rows)` footer. Column widths fit the widest cell — nothing
/// is truncated (this is the deliverable, not a viewport).
pub fn render_console(rows: &Table) -> String {
    let cols = rows.columns();
    if cols.is_empty() {
        return String::new();
    }
    let muted_color = ansi_fg(theme::base::TEXT_MUTED);

    // Width per column: the widest of the header label and every cell (char count, the grid's
    // deterministic width proxy).
    let labels: Vec<String> = cols.iter().map(header_label).collect();
    let widths: Vec<usize> = cols
        .iter()
        .zip(&labels)
        .map(|(col, label)| {
            col.cells
                .iter()
                .map(|c| cell_display(c).chars().count())
                .max()
                .unwrap_or(0)
                .max(label.chars().count())
        })
        .collect();

    let mut out = String::new();

    // Header: bold labels in each column's pastel hue, left-aligned (matching the in-TUI sticky
    // header's per-column coloring).
    let header = labels
        .iter()
        .zip(&widths)
        .enumerate()
        .map(|(idx, (label, &w))| format!("{}{BOLD}{label:<w$}{RESET}", header_fg(idx)))
        .collect::<Vec<_>>()
        .join("  ");
    out.push_str(&header);
    out.push('\n');

    // A dim rule under the header so the table reads as a table in plain scrollback.
    let rule = widths
        .iter()
        .map(|&w| format!("{muted_color}{DIM}{}{RESET}", "\u{2500}".repeat(w)))
        .collect::<Vec<_>>()
        .join("  ");
    out.push_str(&rule);
    out.push('\n');

    for r in 0..rows.row_count() {
        let line = cols
            .iter()
            .zip(&widths)
            .enumerate()
            .map(|(idx, (col, &w))| {
                let cell = &col.cells[r];
                let text = cell_display(cell);
                let padded = if col.ty.is_right_aligned() {
                    format!("{text:>w$}")
                } else {
                    format!("{text:<w$}")
                };
                if cell.is_null() {
                    format!("{muted_color}{DIM}{padded}{RESET}")
                } else {
                    format!("{}{padded}{RESET}", column_fg(idx))
                }
            })
            .collect::<Vec<_>>()
            .join("  ");
        out.push_str(&line);
        out.push('\n');
    }

    let n = rows.row_count();
    out.push_str(&format!(
        "{muted_color}{DIM}({n} row{}){RESET}\n",
        if n == 1 { "" } else { "s" }
    ));
    out
}

#[cfg(test)]
#[path = "console_tests.rs"]
mod console_tests;
