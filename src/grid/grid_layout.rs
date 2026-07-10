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
//! - **Scroll *policy*** (the scroll offsets themselves) is the caller's: it sets
//!   [`GridView::v_row_offset`] / [`GridView::h_col_offset`] / [`GridView::h_char_offset`].
//!   `layout_grid` then materializes only the rows in the visible vertical window
//!   `[v_row_offset, v_row_offset + body_height)` — so a redraw of an uncapped million-row
//!   result formats ~one screenful of cells, not the whole table (the per-keystroke lag before
//!   this was the whole-table format). `GridFrame::body` is therefore the *visible page*, not one
//!   line per table row; its first line is absolute row `v_row_offset`. Column-granular
//!   horizontal scroll is applied here from [`GridView::h_col_offset`] because it is a layout
//!   concern (drop whole leading columns, never slice mid-cell).
//!
//! `GridLayout` is an alias for `GridFrame` (see `dev/DECISIONS.md` S6: canonical name is
//! `GridFrame`). The App lays a result out fresh against the real viewport on every frame, so a
//! `GridFrame` is transient render geometry, not stored state.

use std::ops::Range;

use crate::engine::{Cell, Table};
use crate::schema::ColumnType;

use super::col_width::{MIN_COL_WIDTH, interactive_widths, render_cell};

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
/// the caller passes the already-chosen page).
///
/// **Two horizontal-scroll variables, by design:** `h_col_offset` is column-granular (the
/// keyboard ←/→ axis — predictable, snaps to whole columns); `h_char_offset` is the absolute
/// char-grain slide of the grid (the trackpad axis — smooth). The renderer uses
/// `h_char_offset`; the App keeps both consistent so keyboard nav lands on column boundaries
/// even after a partial trackpad swipe (`h_char_offset = prefix_width(0..h_col_offset)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GridView {
    /// Inner viewport width in terminal cells.
    pub width: u16,
    /// Inner viewport height in terminal cells (rows, header included).
    pub height: u16,
    /// Number of leading columns scrolled off the left edge (column-granular h-scroll;
    /// drives which columns layout_grid emits).
    pub h_col_offset: usize,
    /// Absolute char-grain horizontal slide of the grid (trackpad axis). The render layer
    /// applies `h_char_offset - prefix_width(0..h_col_offset)` chars of `Paragraph::scroll`
    /// to BOTH the header and the body so they slide in lockstep within the leftmost
    /// visible column.
    pub h_char_offset: u16,
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
            h_char_offset: 0,
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
    /// Byte range of each visible column's padded cell within `text`, in visible-column order
    /// (parallel to [`GridFrame::col_x`]). Lets the renderer scan/style per cell — e.g. the
    /// search highlighter matches within cells (never across the gutter) and needs to know
    /// which ranges belong to the current-match column.
    pub cell_spans: Vec<Range<usize>>,
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
    /// The formatted lines for the **visible vertical window** only — one line per table row in
    /// `[body_row_offset, body_row_offset + body.len())`, not one per table row. Materializing
    /// just the on-screen page is what keeps layout O(viewport) against an uncapped result. Each
    /// row carries its line text and the byte ranges that are genuine SQL nulls (so the renderer
    /// dims them without scanning).
    pub body: Vec<BodyRow>,
    /// Absolute table-row index of `body[0]` — i.e. the `v_row_offset` this frame was laid out at.
    /// The blit already slices `body` from its own `v_row_offset`, so with a pre-windowed body it
    /// subtracts this base to index into the page (`body[abs - body_row_offset]`).
    pub body_row_offset: usize,
    /// The start column (0-based, within the rendered frame) of each VISIBLE column, in
    /// visible order. Feeds the schema bar's lockstep alignment and cursor math.
    pub col_x: Vec<u16>,
    /// The **absolute** table-column index of each visible column, in visible order (parallel to
    /// [`col_x`](Self::col_x) / [`widths`](Self::widths) / [`aligns`](Self::aligns) and to each
    /// [`BodyRow::cell_spans`] entry). The renderer keys the per-column pastel hue off the absolute
    /// index so a column keeps its color under horizontal scroll (a purely presentational lookup —
    /// the layout stays color-free, it just says *which* column each visible slot is).
    pub col_indices: Vec<usize>,
    /// Byte range of each visible column's label within [`header`](Self::header), in visible-column
    /// order (parallel to [`col_indices`](Self::col_indices)). Lets the renderer paint each header
    /// label in its column's pastel hue without re-deriving char-vs-byte positions.
    pub header_spans: Vec<Range<usize>>,
    /// The rendered width of each visible column (parallel to `col_x`).
    pub widths: Vec<u16>,
    /// Per-visible-column alignment (parallel to `col_x`).
    pub aligns: Vec<Align>,
    /// Total rendered width (sum of visible widths + gutters) — feeds h-scroll bounds.
    pub total_width: u16,
    /// Horizontal char-grain slide to apply to header + body Paragraphs (`Paragraph::scroll`).
    /// This is the residue of `view.h_char_offset` *within* the leftmost visible column —
    /// i.e. `view.h_char_offset - prefix_width(0..view.h_col_offset)`, clamped to fit the
    /// leftmost visible column's width. Always 0 when h_char_offset matches the column
    /// boundary at h_col_offset (the keyboard-snapped state).
    pub body_scroll_chars: u16,
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
    // Widths come from a fixed row prefix, not the whole table — O(1) in total row count and
    // stable as the user scrolls (see `interactive_widths`).
    let all_widths = interactive_widths(table, view.width.max(MIN_COL_WIDTH));
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
    // `compute_widths` sized each column to fit this label. `header_spans` records each label's
    // byte range so the renderer paints it in the column's pastel hue.
    let (header, header_spans) = join_cells_with_spans(
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

    // Body lines: only the visible vertical window `[body_row_offset, body_end)`, each a join of
    // the visible cells. Formatting just the on-screen page (not every table row) is what keeps
    // this O(viewport) against an uncapped result. The body's height is `view.height` minus the
    // sticky header row the blit reserves. Track which byte ranges came from a genuine `Cell::Null`
    // so the renderer can dim them without re-scanning the text.
    let body_row_offset = view.v_row_offset.min(table.row_count());
    let body_height = view.height.saturating_sub(1) as usize;
    let body_end = body_row_offset
        .saturating_add(body_height)
        .min(table.row_count());
    let body: Vec<BodyRow> = (body_row_offset..body_end)
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

    // Body scroll chars: how far INTO the leftmost visible column the user has trackpad-slid.
    // Subtract the cumulative left-edge X of the leftmost visible column from `h_char_offset`.
    // The App is responsible for keeping `h_col_offset` and `h_char_offset` consistent (every
    // mouse-wheel notch recomputes h_col_offset via `columns_dropped_at`), so the residue always
    // sits within the leftmost visible column's width. Clamp at 0 only as a safety floor.
    let leftmost_x = prefix_left_edge(&all_widths[..start]);
    let body_scroll_chars = view.h_char_offset.saturating_sub(leftmost_x);

    GridFrame {
        header,
        header_spans,
        body,
        body_row_offset,
        col_x,
        col_indices: visible,
        widths,
        aligns,
        total_width,
        body_scroll_chars,
    }
}

/// Cumulative left-edge X (in chars) of the column at index `widths.len()` — i.e. the
/// horizontal position where the *next* column would start, including the trailing gutter
/// after the last column in the slice. `prefix_left_edge(&[])` is 0; `prefix_left_edge(&[w])`
/// is `w + COL_GAP_WIDTH`; `prefix_left_edge(&[a, b])` is `a + COL_GAP_WIDTH + b + COL_GAP_WIDTH`.
/// Pure; the App and the layout share this for char-vs-column conversion: when h_col_offset is
/// `start`, the leftmost visible column begins at x = prefix_left_edge(&widths[..start]).
pub fn prefix_left_edge(widths: &[u16]) -> u16 {
    let mut sum: u16 = 0;
    for w in widths {
        sum = sum.saturating_add(*w).saturating_add(COL_GAP_WIDTH);
    }
    sum
}

/// The largest `k` such that the column at index `k` is fully scrolled off the left edge given
/// `chars` of total horizontal slide — i.e. the start of column `k+1` (= `prefix_left_edge(0..=k)`)
/// is `<= chars`. Used by the mouse handler to recompute the column-granular `h_col_offset`
/// after a trackpad swipe slid `h_char_offset` past one or more whole columns. Pure.
pub fn columns_dropped_at(widths: &[u16], chars: u16) -> usize {
    let mut sum: u16 = 0;
    for (k, w) in widths.iter().enumerate() {
        sum = sum.saturating_add(*w).saturating_add(COL_GAP_WIDTH);
        if sum > chars {
            return k;
        }
    }
    widths.len()
}

/// The new `h_col_offset` that brings column `target` into the viewport with a `margin`-column
/// gap from whichever edge it was off — vim's `scrolloff`, on the horizontal/column axis. Pure so
/// the search "scroll match into view" logic is unit-testable without a terminal.
///
/// `all_widths` are every column's rendered width (from [`compute_widths`]); `viewport` is the
/// inner pane width in cells; `current` is the present `h_col_offset`. The result is clamped to
/// `[0, all_widths.len() - 1]`, and at the data's own left/right end the target is allowed to sit
/// flush against that edge (there is nothing beyond to keep a margin against).
///
/// - Target left of the window (`target < current + margin`): slide left so `margin` columns
///   remain to its left (or to column 0 if the data start is closer).
/// - Target right of the window: slide right so `margin` columns remain to its right — computed by
///   finding the smallest offset that keeps `target + margin` (clamped) visible as the last column,
///   honoring the variable per-column widths.
/// - Target already comfortably inside: `current` unchanged.
pub fn h_col_offset_to_reveal(
    all_widths: &[u16],
    viewport: u16,
    target: usize,
    current: usize,
    margin: usize,
) -> usize {
    let col_count = all_widths.len();
    if col_count == 0 {
        return 0;
    }
    let last = col_count - 1;
    let target = target.min(last);

    // Left edge: the target sits within `margin` columns of the window's left — slide left.
    if target < current.saturating_add(margin) {
        return target.saturating_sub(margin);
    }

    // Right edge: the column we must keep visible is `target + margin` (clamped to the last
    // column). `min_start_showing_last` is the smallest offset at which that column is the last one
    // still fitting; if we're scrolled left of that, slide right to it. Otherwise the target is
    // already inside the window with room to spare, so leave the offset alone.
    let reveal = (target + margin).min(last);
    let need = min_start_showing_last(all_widths, viewport, reveal);
    current.max(need).min(last)
}

/// The smallest `h_col_offset` at which column `target` is still fully visible as (at worst) the
/// last column in the viewport. Accumulates widths leftward from `target` until the next column
/// would overflow `viewport`. Pure helper for [`h_col_offset_to_reveal`].
fn min_start_showing_last(all_widths: &[u16], viewport: u16, target: usize) -> usize {
    let mut start = target;
    let mut used = all_widths.get(target).copied().unwrap_or(0);
    while start > 0 {
        let next = used
            .saturating_add(COL_GAP_WIDTH)
            .saturating_add(all_widths[start - 1]);
        if next > viewport {
            break;
        }
        used = next;
        start -= 1;
    }
    start
}

/// Truncate a plain header string to `width` chars with the same ellipsis rule as cells.
fn render_str(text: &str, width: u16) -> String {
    super::col_width::truncate_to_width(text, width as usize)
}

/// Join already-padded cell strings (for the header line) with the column gutter, recording the
/// byte range of each cell within the joined line. Parallel to the layout's visible-column order,
/// so the renderer can paint each header label in its column's pastel hue.
fn join_cells_with_spans(cells: impl Iterator<Item = String>) -> (String, Vec<Range<usize>>) {
    let mut text = String::new();
    let mut spans: Vec<Range<usize>> = Vec::new();
    for (i, cell) in cells.enumerate() {
        if i != 0 {
            text.push_str(COL_GAP);
        }
        let start = text.len();
        text.push_str(&cell);
        spans.push(start..text.len());
    }
    (text, spans)
}

/// Assemble one [`BodyRow`] from an iterator of `(padded_cell_text, is_null)`, joining with the
/// gutter and recording the byte range of every cell (and separately of each null cell) within
/// the joined text.
fn build_body_row(cells: impl Iterator<Item = (String, bool)>) -> BodyRow {
    let mut text = String::new();
    let mut null_spans: Vec<Range<usize>> = Vec::new();
    let mut cell_spans: Vec<Range<usize>> = Vec::new();
    for (i, (cell, is_null)) in cells.enumerate() {
        if i != 0 {
            text.push_str(COL_GAP);
        }
        let start = text.len();
        text.push_str(&cell);
        cell_spans.push(start..text.len());
        if is_null {
            null_spans.push(start..text.len());
        }
    }
    BodyRow {
        text,
        null_spans,
        cell_spans,
    }
}

#[cfg(test)]
#[path = "grid_layout_tests.rs"]
mod grid_layout_tests;
