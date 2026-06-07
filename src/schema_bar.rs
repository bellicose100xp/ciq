//! Schema bar — the always-visible row of column names + type badges pinned above the grid
//! (`dev/PLAN.md` §6.3).
//!
//! jiq has no equivalent: JSON has no fixed schema, so there is nothing to pin. ciq does — the
//! `Schema` is computed once at load — so a single labeled row showing each column's `name` and a
//! compact ASCII type badge (`int`/`num`/`txt`/`date`/…) gives the data-spelunker the file's shape
//! at a glance. The bar **horizontally scrolls in lockstep with the grid** by sharing the grid's
//! column-granular `h_col_offset` and aligning every entry to the grid's `col_x` so a name sits
//! dead-on over its data column.
//!
//! ## Pure-logic boundary
//!
//! [`layout_schema_bar`] is a pure function of the [`Schema`] and the grid's already-computed
//! [`GridFrame`] geometry (`col_x` + `widths` for the visible columns) — data in, `Vec<Span>` out.
//! No `Frame`, no `Terminal`, no clock, no color decision beyond selecting a named theme style
//! per span. The blit ([`render_schema_bar`]) is the only `Frame`-touching shim and is
//! `TestBackend`-snapshot-tested (NOT shell-exempt). [`summary`] builds the
//! `delim , | header on` indicator string. All three are headless.
//!
//! ## Signature note (re-justified on ciq's merits)
//!
//! §6.3 sketches `layout_schema_bar(&Schema, total_width, h_col_offset, active_col)`. That
//! sketch predates the grid landing; "aligns to the grid's `col_x`" is only achievable by reusing
//! the *exact* per-column widths the grid derived from the data page (a bare `total_width` can't
//! reproduce them — column widths depend on sampled cell content, not the viewport alone). So this
//! takes the grid's [`GridFrame`] directly: it carries `col_x`, `widths`, and the visible-column
//! count, which is the single source of truth for alignment. `h_col_offset` is still passed
//! (it identifies which `Schema` column the first visible grid column maps back to). This is the
//! plan's own "signatures are illustrative; reuse the grid's `col_x`" intent, made exact.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::grid::GridFrame;
use crate::schema::Schema;
use crate::theme;

/// Two spaces between adjacent columns — the same gutter the grid uses, so the bar lines up.
const COL_GAP: &str = "  ";

/// Lay out the schema bar as a list of styled spans, aligned to the grid's `col_x`.
///
/// One entry per **visible** grid column (the grid already applied `h_col_offset` + viewport-fit
/// when it produced `grid`); the visible columns map back to `schema.columns()[h_col_offset..]`.
/// Each entry is `name (badge)` truncated to the column's grid width and left-padded with the
/// gutter so its start byte lands at the column's `col_x`. The active column's entry carries the
/// [`theme::schema_bar::active`] style; the rest carry [`theme::schema_bar::label`].
///
/// `active_col` is an **absolute** column index (into the full schema); it is highlighted only
/// when it falls inside the visible window. Out-of-range or scrolled-off active columns simply
/// render no active span.
///
/// Returns an empty `Vec` when the schema has no columns or the grid shows none.
pub fn layout_schema_bar(
    schema: &Schema,
    grid: &GridFrame,
    h_col_offset: usize,
    active_col: Option<usize>,
) -> Vec<Span<'static>> {
    let columns = schema.columns();
    let visible = grid.col_x.len();
    if columns.is_empty() || visible == 0 {
        return Vec::new();
    }

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(visible * 2);
    for offset in 0..visible {
        let col_idx = h_col_offset + offset;
        let Some(col) = columns.get(col_idx) else {
            break; // grid showed more columns than the schema has (degenerate); stop safely
        };
        let width = grid.widths[offset] as usize;
        let entry = fit_entry(&col.name, col.ty.badge(), width);

        if offset != 0 {
            spans.push(Span::raw(COL_GAP));
        }
        let style = if active_col == Some(col_idx) {
            theme::schema_bar::active()
        } else {
            theme::schema_bar::label()
        };
        spans.push(Span::styled(entry, style));
    }
    spans
}

/// Format one column's bar entry — `name (badge)` — fit to exactly `width` characters: padded
/// with trailing spaces when short, ellipsis-truncated when long. Tries `name (badge)` first; if
/// that overflows, drops the badge and truncates the name alone so a name is always shown over its
/// column even in a narrow viewport.
fn fit_entry(name: &str, badge: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let full = format!("{name} ({badge})");
    if full.chars().count() <= width {
        return pad_right(&full, width);
    }
    // The decorated form doesn't fit; show the (truncated) name alone.
    truncate_to_width(name, width)
}

/// Pad `text` to `width` chars with trailing spaces (assumes `text` already fits).
fn pad_right(text: &str, width: usize) -> String {
    let len = text.chars().count();
    let pad = width.saturating_sub(len);
    format!("{text}{}", " ".repeat(pad))
}

/// Truncate `text` to at most `width` chars with a trailing `…` when cut, then pad to `width`.
/// Mirrors the grid's char-based ellipsis rule (`grid::col_width::truncate_to_width`) so the bar
/// and the grid truncate identically. Never slices a multi-byte char.
fn truncate_to_width(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let len = text.chars().count();
    if len <= width {
        return pad_right(text, width);
    }
    let keep = width.saturating_sub(1);
    let mut out: String = text.chars().take(keep).collect();
    out.push('…');
    out
}

/// The compact delimiter/header indicator shown at the left of the schema-bar context, e.g.
/// `delim , | header on`. Pure: built from the active [`crate::engine::CsvOpts`]-derived dialect
/// the App holds.
///
/// `delimiter` is `None` when DuckDB auto-detected it (shown as `auto`); `header` reflects whether
/// the first row was treated as a header. ASCII only (no emoji); the literal delimiter glyph is
/// shown verbatim (a tab is shown as `\t` so it stays visible).
pub fn summary(delimiter: Option<char>, header: bool) -> String {
    let delim = match delimiter {
        Some('\t') => "\\t".to_string(),
        Some(c) => c.to_string(),
        None => "auto".to_string(),
    };
    let header = if header { "on" } else { "off" };
    format!("delim {delim} | header {header}")
}

/// Blit: paint the schema bar as the single top row of `area`.
///
/// The only `Frame`-touching code in this module. It lays out the bar via [`layout_schema_bar`]
/// against `grid` and renders it as one line at the top of `area` (the row the App reserves above
/// the grid header). No-op when `area` is degenerate. `TestBackend`-snapshot-tested (NOT
/// shell-exempt); colors come from `theme::schema_bar::*` — this file never names a `Color`.
pub fn render_schema_bar(
    f: &mut Frame,
    area: Rect,
    schema: &Schema,
    grid: &GridFrame,
    h_col_offset: usize,
    active_col: Option<usize>,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let spans = layout_schema_bar(schema, grid, h_col_offset, active_col);
    let bar_area = Rect { height: 1, ..area };
    f.render_widget(Paragraph::new(Line::from(spans)), bar_area);
}

#[cfg(test)]
#[path = "schema_bar_tests.rs"]
mod schema_bar_tests;
