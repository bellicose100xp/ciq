//! Pure tabular-grid layout (`dev/PLAN.md` §6.4).
//!
//! `layout_grid(table, &GridView) -> GridFrame` turns a columnar result page plus the
//! viewport/scroll state into the geometry and pre-formatted text the blit shim
//! (`grid_render.rs`) paints. It is **pure**: no `Frame`, no `Terminal`, no clock, no color
//! decision — data in, data out — so an AI agent exercises it headlessly and deterministically.
//!
//! Two things kept out of here on purpose:
//! - **Color/style** lives in the blit (`theme::grid::*`); `GridFrame` carries plain aligned
//!   text plus the per-column alignment so the renderer can style without re-deciding layout.
//! - **Scroll *policy*** (which rows/columns are visible) is the caller's: the vertical row
//!   page is sliced by the caller before calling (jiq's `scroll_offset..end_line` model is
//!   reused unchanged); column-granular horizontal scroll is applied here from
//!   [`GridView::h_col_offset`] because it is a layout concern (drop whole leading columns,
//!   never slice mid-cell).
//!
//! `GridLayout` is an alias for `GridFrame` (see `dev/DECISIONS.md` S6: canonical name is
//! `GridFrame`). The App lays a result out fresh against the real viewport on every frame, so a
//! `GridFrame` is transient render geometry, not stored state.

use std::ops::Range;

use crate::engine::{Cell, Table};
use crate::schema::ColumnType;

use super::col_width::{MIN_COL_WIDTH, compute_widths, render_cell};

/// Two spaces between adjacent columns (the grid's column gutter).
const COL_GAP: &str = "  ";
const COL_GAP_WIDTH: u16 = 2;

/// Per-column horizontal alignment, derived from the column's [`ColumnType`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

impl Align {
    /// Alignment for a column of the given type — numeric/temporal right, everything else
    /// left. Single source of truth is [`ColumnType::is_right_aligned`]; this only adapts it
    /// to the [`Align`] enum.
    pub fn for_type(ty: &ColumnType) -> Self {
        if ty.is_right_aligned() {
            Align::Right
        } else {
            Align::Left
        }
    }
}

/// The viewport / scroll state `layout_grid` needs.
///
/// `width`/`height` are the inner viewport in terminal cells (the blit subtracts the sticky
/// header row from `height` before slicing the body; `layout_grid` itself doesn't slice rows —
/// the caller passes the already-chosen page). `h_col_offset` is the **column-granular**
/// horizontal scroll: that many leading columns are dropped from the left edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GridView {
    /// Inner viewport width in terminal cells.
    pub width: u16,
    /// Inner viewport height in terminal cells (rows, header included).
    pub height: u16,
    /// Number of leading columns scrolled off the left edge (column-granular h-scroll).
    pub h_col_offset: usize,
    /// Number of leading rows scrolled off the top (recorded for the caller's slice math;
    /// `layout_grid` lays out exactly the rows it is given).
    pub v_row_offset: usize,
}

impl GridView {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            h_col_offset: 0,
            v_row_offset: 0,
        }
    }
}

/// One laid-out body row: the joined, aligned line text plus the byte ranges within it that
/// came from a genuine SQL `NULL` cell.
///
/// The renderer styles a `NULL` glyph dimly to mark an absent value. It cannot recover which
/// runs are null by scanning the joined text — a present `Cell::Text("NULL")` (or a value like
/// "ANNULLED") renders the same characters, and a `NULL` glyph truncated below its 4-char width
/// ("N…") no longer contains the literal substring. So null-ness is carried here, from layout,
/// keyed off `Cell::Null` at the source (PLAN.md Q12 null-vs-text distinction).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BodyRow {
    /// The full row line text (visible cells, aligned + padded, joined by the gutter).
    pub text: String,
    /// Byte ranges within `text` (in ascending, non-overlapping order) that render a
    /// `Cell::Null`. Empty when the row has no nulls (the common case).
    pub null_spans: Vec<Range<usize>>,
}

impl BodyRow {
    /// Whether the row line has no text (no visible columns).
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// The produced layout: the sticky header line, the body lines (one per data row), the start
/// column of each visible column, and the total rendered width. Canonical name (S6); also
/// re-exported as the alias [`GridLayout`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridFrame {
    /// The sticky header line text (column names, aligned + gutters), rendered OUTSIDE the
    /// scrolled body by the blit.
    pub header: String,
    /// One formatted line per data row — the `Vec` the existing vertical-slice scroll model
    /// operates on (1 line == 1 row invariant preserved). Each row carries its line text and
    /// the byte ranges that are genuine SQL nulls (so the renderer dims them without scanning).
    pub body: Vec<BodyRow>,
    /// The start column (0-based, within the rendered frame) of each VISIBLE column, in
    /// visible order. Feeds the schema bar's lockstep alignment and cursor math.
    pub col_x: Vec<u16>,
    /// The rendered width of each visible column (parallel to `col_x`).
    pub widths: Vec<u16>,
    /// Per-visible-column alignment (parallel to `col_x`).
    pub aligns: Vec<Align>,
    /// Total rendered width (sum of visible widths + gutters) — feeds h-scroll bounds.
    pub total_width: u16,
}

/// Alias for [`GridFrame`] (`dev/DECISIONS.md` S6). Canonical name is `GridFrame`.
pub type GridLayout = GridFrame;

/// Pad `text` to `width` characters on the side dictated by `align`. `text` is assumed already
/// truncated to `<= width` (it is, via `render_cell`).
fn pad(text: &str, width: u16, align: Align) -> String {
    let len = text.chars().count();
    let pad = (width as usize).saturating_sub(len);
    match align {
        Align::Left => format!("{text}{}", " ".repeat(pad)),
        Align::Right => format!("{}{text}", " ".repeat(pad)),
    }
}

/// Lay out a result page into a [`GridFrame`].
///
/// Pure. Column widths are computed from the page (header + sampled cells, capped to the
/// viewport). Visible columns are those at or after `view.h_col_offset` that still fit the
/// viewport width; each is padded to its width and right/left-aligned by type. The header line
/// and each body line are the visible cells joined by the two-space gutter.
pub fn layout_grid(table: &Table, view: &GridView) -> GridFrame {
    let all_widths = compute_widths(table, view.width.max(MIN_COL_WIDTH));
    let columns = table.columns();

    // Choose visible columns: skip h_col_offset leading columns, then take as many as fit the
    // viewport width (always at least one, so a too-narrow viewport still shows a column).
    let start = view.h_col_offset.min(columns.len());
    let mut visible: Vec<usize> = Vec::new();
    let mut used: u16 = 0;
    for (offset, idx) in (start..columns.len()).enumerate() {
        let w = all_widths[idx];
        let gap = if offset == 0 { 0 } else { COL_GAP_WIDTH };
        let next = used.saturating_add(gap).saturating_add(w);
        if !visible.is_empty() && next > view.width {
            break;
        }
        visible.push(idx);
        used = next;
    }

    let mut col_x: Vec<u16> = Vec::with_capacity(visible.len());
    let mut widths: Vec<u16> = Vec::with_capacity(visible.len());
    let mut aligns: Vec<Align> = Vec::with_capacity(visible.len());
    let mut x: u16 = 0;
    for (offset, &idx) in visible.iter().enumerate() {
        if offset != 0 {
            x = x.saturating_add(COL_GAP_WIDTH);
        }
        col_x.push(x);
        widths.push(all_widths[idx]);
        aligns.push(Align::for_type(&columns[idx].ty));
        x = x.saturating_add(all_widths[idx]);
    }
    let total_width = x;

    // Header line: each column's `name (badge)` label, aligned by type, joined by the gutter.
    // The badge is folded in here so the one sticky header carries the column's sniffed type;
    // `compute_widths` sized each column to fit this label.
    let header = join_cells(
        visible
            .iter()
            .zip(&widths)
            .zip(&aligns)
            .map(|((&idx, &w), &a)| {
                pad(
                    &render_str(&super::col_width::header_label(&columns[idx]), w),
                    w,
                    a,
                )
            }),
    );

    // Body lines: one per row, each a join of the visible cells. Track which byte ranges came
    // from a genuine `Cell::Null` so the renderer can dim them without re-scanning the text.
    let body: Vec<BodyRow> = (0..table.row_count())
        .map(|r| {
            build_body_row(
                visible
                    .iter()
                    .zip(&widths)
                    .zip(&aligns)
                    .map(|((&idx, &w), &a)| {
                        let cell: &Cell = &columns[idx].cells[r];
                        (pad(&render_cell(cell, w as usize), w, a), cell.is_null())
                    }),
            )
        })
        .collect();

    GridFrame {
        header,
        body,
        col_x,
        widths,
        aligns,
        total_width,
    }
}

/// Truncate a plain header string to `width` chars with the same ellipsis rule as cells.
fn render_str(text: &str, width: u16) -> String {
    super::col_width::truncate_to_width(text, width as usize)
}

/// Join already-padded cell strings (for the header line) with the column gutter.
fn join_cells(cells: impl Iterator<Item = String>) -> String {
    let parts: Vec<String> = cells.collect();
    parts.join(COL_GAP)
}

/// Assemble one [`BodyRow`] from an iterator of `(padded_cell_text, is_null)`, joining with the
/// gutter and recording the byte range of each null cell within the joined text.
fn build_body_row(cells: impl Iterator<Item = (String, bool)>) -> BodyRow {
    let mut text = String::new();
    let mut null_spans: Vec<Range<usize>> = Vec::new();
    for (i, (cell, is_null)) in cells.enumerate() {
        if i != 0 {
            text.push_str(COL_GAP);
        }
        let start = text.len();
        text.push_str(&cell);
        if is_null {
            null_spans.push(start..text.len());
        }
    }
    BodyRow { text, null_spans }
}

#[cfg(test)]
#[path = "grid_layout_tests.rs"]
mod grid_layout_tests;
