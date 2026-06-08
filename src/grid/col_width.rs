//! Per-column width computation and cell text rendering for the results grid.
//!
//! Pure, data-in/data-out (`dev/PLAN.md` §6.4): width = `max(header_len, sampled_max_cell_len)`
//! clamped to a per-column cap. Sampling is over the **returned page** (the viewport slice the
//! caller already chose), never the whole table, so this stays O(viewport) and inside the
//! interactive budget. The functions here render a cell to its final on-screen string —
//! ellipsis-truncating when it overflows its column, and substituting a distinct glyph for
//! SQL `NULL` so a null is never confused with an empty-string cell (`Cell::Null` vs
//! `Cell::Text("")`, Q12 rendering distinction).
//!
//! Widths are measured in **character count**, which is the deterministic on-screen-column
//! proxy ciq uses (CSV cell content is overwhelmingly ASCII; no `unicode_width` dependency is
//! taken, keeping this a dependency-free leaf — a wide-glyph refinement can land later without
//! changing the surface).

use crate::engine::{Cell, Column, Table};

/// The glyph rendered for a SQL `NULL` cell — visually distinct from an empty-string cell,
/// which renders as nothing. ASCII only (theme convention, no emoji).
pub const NULL_GLYPH: &str = "NULL";

/// The sticky-header label for a column: `name (badge)`, e.g. `id (int)`, `created_at (date)`.
///
/// Folding the type badge into the one sticky header is what lets the grid show each column's
/// sniffed type inline without a second (visually duplicate) schema-bar row. A column's width is
/// sized to fit this label (see [`compute_widths`]) so the badge is never silently dropped.
pub fn header_label(col: &Column) -> String {
    format!("{} ({})", col.name, col.ty.badge())
}

/// The single-character ellipsis appended when a cell is truncated to fit its column.
pub const ELLIPSIS: char = '…';

/// The default upper bound on any single column's width, so one pathologically wide column
/// can't crowd out every other column. The caller may pass a smaller cap via the viewport
/// budget.
pub const DEFAULT_MAX_COL_WIDTH: u16 = 40;

/// Minimum width any visible column is given (so a 1-char column still shows its ellipsis /
/// at least one content char). Header text shorter than this still occupies this floor.
pub const MIN_COL_WIDTH: u16 = 1;

/// The string a cell renders to *before* truncation: the null glyph for `Cell::Null`, the
/// cell's own display text otherwise (empty string for `Cell::Text("")`).
///
/// This is the one place the null-vs-empty distinction is made: `Cell::Null -> "NULL"`,
/// `Cell::Text("") -> ""`.
pub fn cell_display(cell: &Cell) -> String {
    match cell {
        Cell::Null => NULL_GLYPH.to_string(),
        other => other.display(),
    }
}

/// The number of characters `cell` renders to (its unclamped natural width).
pub fn cell_char_len(cell: &Cell) -> usize {
    cell_display(cell).chars().count()
}

/// Truncate `text` to at most `width` characters, replacing the dropped tail with a single
/// ellipsis when truncation occurs. `width == 0` yields the empty string. This is the common
/// cell case (tail truncation); it never panics on multi-byte input because it iterates by
/// `char`, never slicing on a byte boundary.
pub fn truncate_to_width(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let char_len = text.chars().count();
    if char_len <= width {
        return text.to_string();
    }
    // Keep `width - 1` chars and append the ellipsis (which occupies the last column).
    let keep = width.saturating_sub(1);
    let mut out: String = text.chars().take(keep).collect();
    out.push(ELLIPSIS);
    out
}

/// Render a cell to its final on-screen text for a column of `width` columns: substitute the
/// null glyph (for `Cell::Null`), then ellipsis-truncate to `width`.
pub fn render_cell(cell: &Cell, width: usize) -> String {
    truncate_to_width(&cell_display(cell), width)
}

/// Compute the per-column display widths for a result page.
///
/// For each column: `width = clamp(max(header_label_chars, max_sampled_cell_chars), MIN, max_cap)`.
/// The header term is the full `name (badge)` label ([`header_label`]), not the bare name, so the
/// type badge in the sticky header always fits. `max_cap` is the smaller of
/// [`DEFAULT_MAX_COL_WIDTH`] and `viewport_budget` (a single column never exceeds the whole
/// viewport). The returned vector has one entry per column in `table` order. An empty table
/// yields header-only widths.
pub fn compute_widths(table: &Table, viewport_budget: u16) -> Vec<u16> {
    let cap = DEFAULT_MAX_COL_WIDTH.min(viewport_budget.max(MIN_COL_WIDTH));
    table
        .columns()
        .iter()
        .map(|col| {
            let header = header_label(col).chars().count();
            let widest_cell = col.cells.iter().map(cell_char_len).max().unwrap_or(0);
            let natural = header.max(widest_cell) as u16;
            natural.clamp(MIN_COL_WIDTH, cap)
        })
        .collect()
}

#[cfg(test)]
#[path = "col_width_tests.rs"]
mod col_width_tests;
