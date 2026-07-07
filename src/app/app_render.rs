//! App render layer — results grid (top) + query bar + status line (both bottom).
//!
//! `dev/PLAN.md` §3 / §4.1: a **pure function of `App` state into a `Frame`**. The only
//! `Frame`-touching code in the shell besides the grid blit, and like every ciq render surface
//! it is `TestBackend`-snapshot-tested (NOT shell-exempt — `TestBackend` is an in-memory cell
//! grid an agent asserts). All colors come from [`theme::app`] / [`theme::grid`]; this file
//! never names a `Color`.
//!
//! Layout (top -> bottom): a bordered results pane filling the space (which re-lays-out the
//! retained result `rows` against the *actual* inner viewport so a resize reflows without
//! re-querying — §3.1's "App re-lays-out from retained rows on resize"), then a **bordered query
//! box** near the bottom whose **bottom border carries the context-sensitive keyboard help hints**
//! (§4.1, jiq-style — the hints live on the box border, not a standalone row), then a one-row
//! status line at the very bottom. The query *input* sits near the bottom of the screen; popups
//! anchor just **above** the query box, over the results pane.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::ai::ai_render::render_ai;
use crate::app::help_line;
use crate::app::layout_regions::{LayoutRegions, PopupKind};
use crate::app::{App, AppPhase, Focus, QueryMode, SimplePane};
use crate::autocomplete::autocomplete_render::{MAX_VISIBLE_ROWS, render_popup};
use crate::facets::facet_render::render_facet;
use crate::grid::{GridView, grid_render, layout_grid};
use crate::history::history_render::render_history;
use crate::history::history_state::MAX_VISIBLE_HISTORY;
use crate::ingest::dialect_summary;
use crate::palette::palette_render::{MAX_VISIBLE_ROWS as PALETTE_MAX_ROWS, render_palette};
use crate::theme;

/// Max content rows the facet popup reserves: the histogram is up to 2 stat lines + the top-K bars
/// ([`DEFAULT_TOP_K`](crate::facets::facet_query::DEFAULT_TOP_K) = 10), so 12 covers the largest
/// case; a summary (4 lines) fits comfortably inside it. The popup is height-clamped to the pane.
const FACET_POPUP_ROWS: u16 = 12;

/// How many *text* rows the query box may grow to as the query gains lines (the multiline policy).
/// A single-line query keeps one text row; each additional line adds a row up to this cap, after
/// which the textarea scrolls within the fixed window. Chosen small so a long query never crowds
/// out the results pane. The box itself is 2 rows taller (top + bottom border) — see [`box_height`].
const MAX_BAR_ROWS: u16 = 5;

/// The number of editable text rows for the current query: one per line in Power, clamped to
/// [1, [`MAX_BAR_ROWS`]]. In Simple mode the bar is a fixed five-pane form (one row per pane),
/// independent of the focused pane's content — the panes are always visible.
fn bar_text_rows(app: &App) -> u16 {
    match app.query_form().mode() {
        QueryMode::Simple => SIMPLE_PANE_COUNT as u16,
        QueryMode::Power => (app.editor().line_count() as u16).clamp(1, MAX_BAR_ROWS),
    }
}

/// The total height of the bordered query box: the text rows plus the top + bottom border rows.
/// The bottom border carries the keyboard help hints (§4.1).
fn box_height(app: &App) -> u16 {
    bar_text_rows(app).saturating_add(2)
}

/// How many panes the Simple-mode query form has (`SELECT` / `WHERE` / `GROUP BY` / `ORDER BY` /
/// `LIMIT`). The bar reserves exactly this many text rows in Simple mode.
const SIMPLE_PANE_COUNT: usize = 5;

/// Render the whole app into `frame`.
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();
    // When the Ctrl+F search bar is open it takes a fixed-height row between the results pane
    // and the query box (jiq's placement); closed, the results pane reclaims the space.
    let search_height = if app.search().is_visible() {
        crate::search::search_render::SEARCH_BAR_HEIGHT
    } else {
        0
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),                  // results pane (fills the space)
            Constraint::Length(search_height),   // Ctrl+F search bar (0 when closed)
            Constraint::Length(box_height(app)), // bordered query box (text rows + 2 border rows)
            Constraint::Length(1),               // status line (very bottom)
        ])
        .split(area);

    let results = chunks[0];
    let query_box = chunks[2];

    render_results(app, frame, results);
    if app.search().is_visible() {
        let shown = app.display_rows().map(|r| r.row_count()).unwrap_or(0);
        let total = app.result().map(|r| r.rows.row_count()).unwrap_or(0);
        crate::search::search_render::render_search_bar(
            frame,
            chunks[1],
            app.search().needle(),
            app.search().is_confirmed(),
            shown,
            total,
        );
    }
    // The query box is bordered; its bottom border carries the help hints. `render_query_box`
    // returns the inner text rect — what the editor occupies and what a mouse click maps onto.
    let text_area = render_query_box(app, frame, query_box);
    render_status(app, frame, chunks[3]);
    // The popups overlay the results pane, anchored just ABOVE the bottom query box (drawn last so
    // they sit on top). Headless: still just cells in the TestBackend buffer. They are mutually
    // exclusive (opening one closes the others), so painting them all is at most one visible box.
    render_autocomplete(app, frame, query_box, results);
    render_palette_popup(app, frame, query_box, results);
    render_facet_popup(app, frame, query_box, results);
    render_history_popup(app, frame, query_box, results);
    render_ai_popup(app, frame, query_box, results);

    // Record the on-screen regions so the next mouse event resolves against the geometry the user
    // actually sees (the click-to-focus / click-to-position / scroll-routing seam). The query-bar
    // region is the box's INNER text area (border-stripped), so the existing prompt/text-col mouse
    // math is unchanged. At most one popup is open (mutually exclusive overlays), recomputed with
    // the same `popup_above_bar` anchoring the render used above.
    app.set_layout_regions(LayoutRegions {
        results_pane: Some(results),
        query_bar: Some(text_area),
        popup: active_popup_region(app, query_box, results),
    });
}

/// The kind + on-screen rect of the single open popup (mutually exclusive overlays), or `None`
/// when none is open. The single seam that owns popup sizing — every `render_*_popup` asks the
/// same helper for its rect, so the recorded `LayoutRegions::popup` rect always matches what was
/// drawn. (Pre-DRY this duplicated five `popup_above_bar(bar, results, rows)` calls; a mismatch
/// already caused a real mouse-mis-routing bug — see commit 6aa948b.)
fn active_popup_region(app: &App, bar: Rect, results: Rect) -> Option<(PopupKind, Rect)> {
    let kind = active_popup_kind(app)?;
    Some((kind, popup_rect_for(app, kind, bar, results)))
}

/// The single open popup's kind, or `None` when none is open. Mutually exclusive — opening one
/// closes the others.
fn active_popup_kind(app: &App) -> Option<PopupKind> {
    if app.is_ai_open() {
        return Some(PopupKind::Ai);
    }
    if app.is_history_open() {
        return Some(PopupKind::History);
    }
    if app.is_palette_open() && app.palette().is_some() {
        return Some(PopupKind::Palette);
    }
    if app.facet().is_some() {
        return Some(PopupKind::Facet);
    }
    if app.autocomplete().is_open() {
        return Some(PopupKind::Autocomplete);
    }
    None
}

/// The on-screen rect for the popup of `kind`. Sizing is per-kind: AI is a fixed 2-row prompt
/// box; history/palette/autocomplete size to the filtered count (so a narrowed list doesn't
/// leave a blank lower band); facet uses [`FACET_POPUP_ROWS`]. The anchoring is uniform via
/// [`popup_above_bar`].
fn popup_rect_for(app: &App, kind: PopupKind, bar: Rect, results: Rect) -> Rect {
    let rows = match kind {
        PopupKind::Ai => 2,
        PopupKind::History => {
            (app.history().filtered_count().max(1) as u16).min(MAX_VISIBLE_HISTORY as u16)
        }
        PopupKind::Palette => app
            .palette()
            .map(|p| (p.all_columns().len().max(1) as u16).min(PALETTE_MAX_ROWS))
            .unwrap_or(1),
        PopupKind::Facet => FACET_POPUP_ROWS,
        PopupKind::Autocomplete => (app.autocomplete().len() as u16).min(MAX_VISIBLE_ROWS),
    };
    // The column-picker floors its width to fit its full bottom-border hint line (incl. the
    // Ctrl+A/Ctrl+X/Ctrl+I bulk ops) — the box is wider than other popups so those hints always
    // show. Clamped to the pane so a genuinely tiny terminal still can't overflow.
    let min_width = match kind {
        PopupKind::Palette => (crate::palette::palette_render::hint_line_width() as u16)
            .saturating_add(2)
            .min(results.width),
        _ => 0,
    };
    popup_above_bar(bar, results, rows, min_width)
}

/// The screen rectangle for a popup of `content_rows` content rows + a border, anchored so its
/// **bottom** edge sits just above the query `bar` and it grows upward into the `results` pane.
/// Height is clamped to the available results height; the box never overflows the top of the pane.
/// `min_width` floors the box width (clamped to the pane by the caller) so a popup whose chrome
/// needs more room than the default — e.g. the column picker's bulk-op hint line — gets it.
fn popup_above_bar(bar: Rect, results: Rect, content_rows: u16, min_width: u16) -> Rect {
    let height = (content_rows + 2).min(results.height.max(1)); // +2 for the popup border
    // Anchor the box's bottom edge on the row directly above the bar, growing upward.
    let bottom = bar.y; // exclusive bottom (the bar row itself stays visible)
    let y = bottom.saturating_sub(height).max(results.y);
    let width = popup_width(results.width).max(min_width);
    Rect {
        x: bar.x,
        y,
        width,
        height,
    }
}

/// Overlay the AI NL->SQL popup just above the query bar, over the results pane, when open (P5.1).
/// A short fixed-height box (the prompt line + a status line). No-op when the popup is closed.
fn render_ai_popup(app: &App, frame: &mut Frame, bar: Rect, results: Rect) {
    if !app.is_ai_open() {
        return;
    }
    let area = popup_rect_for(app, PopupKind::Ai, bar, results);
    render_ai(app.ai(), frame, area);
}

/// Overlay the history popup just above the query bar, over the results pane, when open (P5.2).
/// Sized to the filtered entry count (capped by the visible-row window and the available height)
/// and to a readable fraction of the width. No-op when the popup is closed.
fn render_history_popup(app: &App, frame: &mut Frame, bar: Rect, results: Rect) {
    if !app.is_history_open() {
        return;
    }
    let area = popup_rect_for(app, PopupKind::History, bar, results);
    render_history(
        app.history(),
        frame,
        area,
        hovered_popup_row(app, PopupKind::History),
    );
}

/// Overlay the facet popup just above the query bar, over the results pane, when one is open
/// (P4.6). Sized to the stat/histogram line count (capped by the available height) and to a
/// readable fraction of the width. No-op when no facet is open.
fn render_facet_popup(app: &App, frame: &mut Frame, bar: Rect, results: Rect) {
    let Some(facet) = app.facet() else {
        return;
    };
    let area = popup_rect_for(app, PopupKind::Facet, bar, results);
    render_facet(facet, frame, area);
}

/// Overlay the column palette just above the query bar, over the results pane, when it is open.
/// Sized to the column count (capped by the palette's visible-row window and the available height)
/// and to a readable fraction of the width. No-op when the palette is closed.
fn render_palette_popup(app: &App, frame: &mut Frame, bar: Rect, results: Rect) {
    if !app.is_palette_open() {
        return;
    }
    let Some(palette) = app.palette() else {
        return;
    };
    let area = popup_rect_for(app, PopupKind::Palette, bar, results);
    render_palette(
        palette,
        frame,
        area,
        hovered_popup_row(app, PopupKind::Palette),
    );
}

/// Overlay the autocomplete popup just above the query bar, over the results pane. Sized to the
/// candidate count (capped by [`MAX_VISIBLE_ROWS`] and the available height) and to a readable
/// fraction of the width. No-op when the popup is closed (handled inside `render_popup`).
fn render_autocomplete(app: &App, frame: &mut Frame, bar: Rect, results: Rect) {
    let state = app.autocomplete();
    if !state.is_open() {
        return;
    }
    let area = popup_rect_for(app, PopupKind::Autocomplete, bar, results);
    // The `Ctrl+P columns` hint surfaces only when focus is on the SELECT pane (Simple mode) — the
    // chord is anchored to that pane, and revealing it on the autocomplete popup gives the user
    // the discoverable jump to the dedicated column-picker palette.
    let show_columns_hint = matches!(app.query_form().mode(), crate::app::QueryMode::Simple)
        && app.query_form().focused_pane() == crate::app::SimplePane::Select;
    render_popup(
        state,
        frame,
        area,
        show_columns_hint,
        hovered_popup_row(app, PopupKind::Autocomplete),
    );
}

/// The absolute list index the pointer is hovering inside popup `kind`, if any — feeds each list
/// popup's hover band. `None` when the hover is on another surface (or nothing).
fn hovered_popup_row(app: &App, kind: PopupKind) -> Option<usize> {
    match app.hover() {
        Some(crate::app::HoverTarget::PopupRow(k, row)) if k == kind => Some(row),
        _ => None,
    }
}

/// The popup width: a readable fraction of the pane width, clamped so it neither overflows nor
/// shrinks below a usable minimum.
fn popup_width(pane_width: u16) -> u16 {
    const MIN: u16 = 16;
    const MAX: u16 = 40;
    pane_width.clamp(MIN.min(pane_width.max(1)), MAX.min(pane_width.max(1)))
}

/// Width of the leading `> ` prompt column reserved at the left of the query bar. The single source
/// of truth for the prompt offset: the render uses it to place the textarea, and
/// [`App::on_mouse`](crate::app::App::on_mouse) uses it to map a query-bar click column onto the
/// editable text (subtracting the prompt).
pub(crate) const PROMPT_WIDTH: u16 = 2;

/// The query box: a bordered [`Block`] whose **bottom border carries the context-sensitive keyboard
/// help hints** (§4.1, jiq-style), wrapping a `> ` prompt glyph in a fixed left column + the
/// multiline editing textarea (which paints its own visible cursor cell into the buffer —
/// headless-snapshotable). The prompt pins to the top inner row so it reads as a single baseline
/// marker even when the textarea spans rows. Returns the box's **inner rect** (border-stripped,
/// before the prompt) so the caller records it as the mouse-click target — [`text_col`] subtracts
/// the `> ` prompt from it, so a click on the prompt clamps to column 0 and a click on the text
/// maps onto the right character (the click math is unchanged from the borderless bar, just shifted
/// in by the border). Returns a zero-size rect on a degenerate area.
fn render_query_box(app: &App, frame: &mut Frame, area: Rect) -> Rect {
    if area.width == 0 || area.height == 0 {
        return Rect::new(area.x, area.y, 0, 0);
    }
    // Borders are STATE-aware (jiq-style): the query box's border tracks the focused vim mode
    // (Insert=cyan, Normal=yellow, Operator/TextObject=green, CharSearch=pink), with a query
    // pipeline error overriding the mode color (red). Unfocused recedes in muted slate. The mode
    // badge on the top border, the keys on the bottom-border hint line, and the border itself all
    // share the same accent so the box reads as one harmonized state indicator.
    let focused = app.focus() == Focus::QueryBar;
    let mode = app.editor_mode();
    let has_error = app.has_query_error();
    let border_style = theme::border::query_box(mode, has_error, focused);
    // Accent for badges/hints — same hue as the border. When unfocused (or no border accent
    // applies) we leave the keys in the muted slate by passing the unfocused color, matching the
    // border. When focused with no error, the key color is the mode color; with an error, red.
    let accent = if !focused {
        theme::base::BORDER_UNFOCUSED
    } else if has_error {
        theme::base::BORDER_ERROR
    } else {
        theme::border::mode_color(mode)
    };
    // The mode badge rides the box's TOP border (jiq-style) — left-aligned, per-mode color.
    // The bottom border carries the context-sensitive help hints, CENTERED so the legend reads
    // as one compact unit — but **only when the query bar is focused**, so the hints sit on the
    // box that actually owns them. When the results pane is focused, [`render_results`] paints
    // the hints on its own bottom border instead, leaving this border empty. Width-aware: a
    // narrow box drops trailing hints rather than overflowing (the usable title width is the
    // box width minus the two corner glyphs).
    let title_width = area.width.saturating_sub(2) as usize;
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style);
    if focused {
        let hint_spans = help_line::hint_spans_in(app, title_width, accent);
        block = block.title_bottom(Line::from(hint_spans).centered());
    }
    if let Some(label) = help_line::mode_label(app) {
        // The badge text fits when the label width <= title width; otherwise we drop it rather
        // than clip mid-word (the box is too narrow to be useful anyway). Badge color matches the
        // border accent so the harmony reads even when the badge is the only thing on the row.
        if label.chars().count() <= title_width {
            let badge_style = Style::default().fg(accent).add_modifier(Modifier::BOLD);
            let badge = Span::styled(label, badge_style);
            block = block.title_top(Line::from(vec![Span::raw(" "), badge, Span::raw(" ")]));
        }
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return Rect::new(inner.x, inner.y, 0, 0);
    }

    match app.query_form().mode() {
        QueryMode::Simple => render_simple_panes(app, frame, inner),
        QueryMode::Power => render_power_textarea(app, frame, inner),
    }
    // Record the INNER rect (before the prompt) as the click target: `text_col` subtracts the
    // prompt from it, matching the borderless-bar contract the mouse mapping was written against.
    // In Simple mode the per-pane row mapping uses the inner rect's `y` as the origin too —
    // `MouseTarget::QueryBar { row, .. }` is `y - inner.y`, which the App reads as the pane index.
    inner
}

/// Power-mode bar render: a `> ` prompt in the fixed left column and the textarea in the rest.
fn render_power_textarea(app: &App, frame: &mut Frame, inner: Rect) {
    let prompt_w = PROMPT_WIDTH.min(inner.width);
    let prompt_area = Rect {
        height: 1,
        width: prompt_w,
        ..inner
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled("> ", theme::app::prompt()))),
        prompt_area,
    );
    let text_area = Rect {
        x: inner.x.saturating_add(prompt_w),
        width: inner.width.saturating_sub(prompt_w),
        ..inner
    };
    if text_area.width > 0 {
        frame.render_widget(app.editor().textarea(), text_area);
    }
}

/// Simple-mode bar render: 5 stacked single-line panes, one per [`SimplePane`], with a fixed-width
/// label column on the left of each row. Only the focused pane's textarea paints its cursor cell;
/// the others render as plain text rows. The pane order is the canonical SELECT / WHERE / GROUP BY
/// / ORDER BY / LIMIT — matching `SimplePane::ALL`.
fn render_simple_panes(app: &App, frame: &mut Frame, inner: Rect) {
    // Need at least one row to render anything; we silently truncate if the box is shorter than
    // five rows (the layout sizes box_height to 7 rows in Simple mode, so this is defensive).
    if inner.height == 0 {
        return;
    }
    let label_w = SIMPLE_LABEL_WIDTH.min(inner.width);
    let panes = [
        SimplePane::Select,
        SimplePane::Where,
        SimplePane::GroupBy,
        SimplePane::OrderBy,
        SimplePane::Limit,
    ];
    let focused = app.query_form().focused_pane();
    for (i, &pane) in panes.iter().enumerate() {
        let i = i as u16;
        if i >= inner.height {
            break;
        }
        let row = Rect {
            x: inner.x,
            y: inner.y.saturating_add(i),
            width: inner.width,
            height: 1,
        };
        let is_focused = pane == focused;
        // The focused pane is marked, lazygit-style, with a bright left accent bar (`▌`) over a
        // faint MODE-TINTED background band (cyan Insert / yellow Normal / … / red on a query
        // error) — cohesive with the box's mode-aware border, not a flat neutral fill. The bar
        // sits in column 0 and the label shifts one cell right, so the editor text still starts at
        // `label_w` and the mouse click→text-col mapping is unchanged.
        let accent = theme::border::query_box_accent(app.editor_mode(), app.has_query_error());
        let row_bg = if is_focused {
            theme::app::active_pane_bg(accent)
        } else {
            Style::default()
        };
        if is_focused {
            // Tint the whole row first, then overlay the accent bar in column 0.
            frame.render_widget(Block::default().style(row_bg), row);
            let bar_area = Rect { width: 1, ..row };
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "\u{258c}",
                    theme::app::active_pane_bar(accent),
                )),
                bar_area,
            );
        }
        // The label starts at column 1 when focused (column 0 holds the accent bar), else column 0.
        let label_x_off = if is_focused { 1 } else { 0 };
        let label_area = Rect {
            x: row.x.saturating_add(label_x_off),
            width: label_w.saturating_sub(label_x_off),
            ..row
        };
        let label_style = if is_focused {
            theme::app::pane_label_focused().patch(row_bg)
        } else {
            theme::app::pane_label()
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(pane.label(), label_style))).style(row_bg),
            label_area,
        );
        let text_x = inner.x.saturating_add(label_w);
        let text_area = Rect {
            x: text_x,
            y: row.y,
            width: inner.width.saturating_sub(label_w),
            height: 1,
        };
        if text_area.width == 0 {
            continue;
        }
        // Only the focused pane shows a cursor. tui-textarea paints its cursor cell into the
        // buffer (the visible block-cursor is reverse-video — see `theme::app::cursor`), and that
        // style is per-textarea state. Rendering the focused pane's `&TextArea` directly keeps
        // its mode-driven cursor color; for unfocused panes we render a clone with the cursor
        // style stripped so no extra cursor cells appear elsewhere in the bar.
        if is_focused {
            // Clone so the subtle active-pane background can be applied to the textarea's cells
            // without mutating the stored pane state; the cursor style is preserved.
            let mut focused_ta = app.query_form().pane(pane).textarea().clone();
            focused_ta.set_style(row_bg);
            frame.render_widget(&focused_ta, text_area);
        } else {
            let mut cloned = app.query_form().pane(pane).textarea().clone();
            cloned.set_cursor_style(theme::app::cursor_suppressed());
            frame.render_widget(&cloned, text_area);
        }
    }
}

/// Width of the left label column in a Simple-mode pane row (`SELECT  `, `WHERE   `, `GROUP BY`,
/// `ORDER BY`, `LIMIT   `). 9 fits the longest label (`GROUP BY` / `ORDER BY` = 8) plus a single
/// space gutter before the editor text. Kept `pub(crate)` so the mouse click→text-col mapping in
/// [`super::layout_regions::LayoutRegions::text_col`] subtracts the same offset.
pub(crate) const SIMPLE_LABEL_WIDTH: u16 = 9;

/// The results pane: a bordered box containing the aligned grid, a loading indicator, or an
/// empty hint. The grid is re-laid-out against the actual inner viewport. The pane border is
/// focus-aware (bright cyan when the results pane has focus, muted slate otherwise) and carries
/// two pieces of metadata: the CSV dialect summary on the top-left, and a row counter on the
/// top-right (`<rendered>` for non-capped, `<rendered>+` when ciq's viewport `LIMIT` truncated
/// the grid) so the grid size reads at a glance without consuming an interior row. The counter
/// is omitted on a zero-row result so it doesn't duplicate the "no rows match" empty-state line.
fn render_results(app: &App, frame: &mut Frame, area: Rect) {
    // Border is STATE-aware (jiq-style): green for a successful result with rows, yellow for a
    // zero-row success, red for an error (the prior result is dimmed), cyan while pending. The
    // matching accent is reused by the row counter and the bottom-hint key spans so the whole
    // pane chrome reads as one verdict.
    let focused = app.focus() == Focus::Results;
    let state = app.result_state();
    let border_style = theme::border::results(state, focused);
    let accent = theme::border::result_color(state);
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style);
    // Once loaded, surface the CSV dialect (`delim , | header on`) as the pane's border title —
    // the global indicator §6.3 calls for, distinct from the per-column schema bar below.
    if app.schema().is_some() {
        let (delim, header) = app.csv_summary();
        block = block.title(Span::styled(
            dialect_summary(delim, header),
            theme::app::dialect_summary(),
        ));
    }
    // The row counter rides the top-right of the border: `<rendered>` for an uncapped result,
    // `<rendered>+` when ciq's viewport LIMIT truncated the grid. Omitted on a zero-row result
    // so the counter doesn't duplicate the "no rows match" empty-state body. Stale results get
    // the muted styling so the counter dims with the grid; otherwise the counter takes the same
    // state accent as the border so the verdict reads at a glance.
    if let Some(text) = row_counter_text(app) {
        let style = if app.result_is_stale() {
            theme::results::row_counter_stale()
        } else {
            theme::results::row_counter_in(accent)
        };
        block = block.title_top(Line::from(Span::styled(text, style)).right_aligned());
    }
    // The bottom border carries the Results-pane keyboard hints (`Up/Down scroll`,
    // `Ctrl+T query`, …) when this pane is focused, mirroring the query box's bottom border. Each
    // box owns its own hints so it's visually unambiguous which chord set applies. Width-aware so
    // a narrow pane drops trailing hints rather than overflowing. Key color is the state accent so
    // the chord names harmonize with the border (jiq's `border_color` plumbed into the hint
    // builder).
    if focused {
        let title_width = area.width.saturating_sub(2) as usize;
        let hint_spans = help_line::hint_spans_in(app, title_width, accent);
        block = block.title_bottom(Line::from(hint_spans).centered());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    match app.phase() {
        AppPhase::Loading => {
            let p = Paragraph::new(Span::styled("loading CSV…", theme::app::loading()));
            frame.render_widget(p, inner);
            return;
        }
        AppPhase::LoadError(msg) => {
            let p = Paragraph::new(Span::styled(
                format!("could not load CSV: {msg}"),
                theme::app::status_error(),
            ));
            frame.render_widget(p, inner);
            return;
        }
        AppPhase::Ready | AppPhase::Querying => {}
    }

    // An empty result (zero rows) or no result yet shows the empty-state line, never the grid.
    if let Some(message) = app.empty_state() {
        let p = Paragraph::new(Span::styled(message, theme::app::empty_state()));
        frame.render_widget(p, inner);
        return;
    }

    // The displayed rows: the Ctrl+F-filtered projection while a search is active, else the full
    // result — one seam (`App::display_rows`) so the grid, scroll bounds, and counters agree.
    if let Some(rows) = app.display_rows() {
        // The grid claims the full inner pane (no reserved interior row for a banner) — the
        // row counter on the border carries the cap signal instead.
        let view = GridView {
            width: inner.width,
            height: inner.height,
            h_col_offset: app.h_col_offset(),
            h_char_offset: app.h_char_offset(),
            v_row_offset: app.v_row_offset(),
        };
        let grid = layout_grid(rows, &view);
        // The hover band only paints while the pointer is on a grid row (popup hover paints in
        // the popup renderers instead).
        let hovered_row = match app.hover() {
            Some(crate::app::HoverTarget::GridRow(row)) => Some(row),
            _ => None,
        };
        let needle = if app.search().is_filtering() {
            app.search().needle()
        } else {
            ""
        };
        grid_render::render_grid(
            frame,
            inner,
            &grid,
            grid_render::GridPaint {
                v_row_offset: app.v_row_offset(),
                stale: app.result_is_stale(),
                hovered_row,
                accent,
                search_needle: needle,
            },
        );
    }
}

/// The row counter shown on the top-right of the results pane border. Returns `None` until a
/// result is on screen, and `None` for a zero-row result (the empty-state body — "no rows match"
/// — is the canonical zero signal; reinforcing it on the border would just duplicate it).
/// When the grid was ciq-capped (the displayed result's query was wrapped in the viewport
/// `LIMIT`), the count carries a `+` suffix so the cap reads without occupying an interior row —
/// `1000+` for a capped grid, `12` for an uncapped one. The cap-suffix path is unreachable on
/// `rendered == 0` because truncation requires `rendered >= cap > 0`.
fn row_counter_text(app: &App) -> Option<String> {
    // Displayed (possibly Ctrl+F-filtered) rows — the counter describes what's on screen; the
    // search bar's own shown/total badge carries the filter arithmetic.
    let rendered = app.display_rows()?.row_count();
    if rendered == 0 {
        return None;
    }
    if app.truncation_banner().is_some() {
        Some(format!("{rendered}+"))
    } else {
        Some(format!("{rendered}"))
    }
}

/// The status line: the status text (error-styled on a load error, normal otherwise) at the left.
/// The vim mode badge (`INSERT` / `NORMAL` / `d(` …) leads the help hints on the query box's bottom
/// border, so the mode is always visible without duplicating it here.
fn render_status(app: &App, frame: &mut Frame, area: Rect) {
    // Red for a fatal load failure OR a live query error (`unknown column`, a bad LIMIT, an engine
    // error that dimmed the last result) — the same signal that reddens the query-box border, so
    // the message and the border agree.
    let style = if matches!(app.phase(), AppPhase::LoadError(_)) || app.has_query_error() {
        theme::app::status_error()
    } else {
        theme::app::status()
    };
    frame.render_widget(
        Paragraph::new(Span::styled(app.status().to_string(), style)),
        area,
    );
}

#[cfg(test)]
#[path = "app_render_tests.rs"]
mod app_render_tests;
