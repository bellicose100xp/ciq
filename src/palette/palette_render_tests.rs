//! Tests for `palette_render` — the pure row helpers (`row_text`/`checkbox`) and the popup blit
//! (logical-cell snapshots via `ratatui::TestBackend`).
//!
//! The snapshot proves the *logical* cell grid (which checkboxes / column names / right-aligned
//! type badges land where) and that the bottom-border hints render. True-terminal glyphs, popup
//! placement against a real screen, and the magenta accent color are the §4.7 human surface, NOT
//! asserted here.

use super::*;
use crate::palette::palette_state::{ColumnRef, PaletteState};
use crate::schema::ColumnType;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

fn cols() -> Vec<ColumnRef> {
    vec![
        ColumnRef::new("id", ColumnType::Int),
        ColumnRef::new("status", ColumnType::Text),
        ColumnRef::new("amount", ColumnType::Float),
        ColumnRef::new("created_at", ColumnType::Date),
    ]
}

fn render(state: &PaletteState, w: u16, h: u16, area: Rect) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).expect("TestBackend");
    t.draw(|f| render_palette(state, f, area, None))
        .expect("draw");
    t.backend().to_string()
}

// --- pure row helpers ---

#[test]
fn checkbox_glyphs() {
    assert_eq!(checkbox(true), "[x]");
    assert_eq!(checkbox(false), "[ ]");
}

#[test]
fn row_text_shows_checkbox_and_name() {
    let c = ColumnRef::new("status", ColumnType::Text);
    assert_eq!(row_text(&c, false, 20), "[ ] status");
    assert_eq!(row_text(&c, true, 20), "[x] status");
}

#[test]
fn row_text_truncates_long_names_with_ellipsis() {
    let c = ColumnRef::new("a_very_long_column_name", ColumnType::Text);
    let txt = row_text(&c, false, 10);
    assert_eq!(txt.chars().count(), 10);
    assert!(txt.ends_with('\u{2026}'), "got: {txt}");
}

// --- render ---

#[test]
fn popup_shows_columns_checkboxes_and_badges() {
    let mut state = PaletteState::new(cols());
    state.toggle(0); // id checked
    state.toggle(2); // amount checked
    let screen = render(&state, 40, 12, Rect::new(0, 0, 30, 8));
    // Column names present.
    assert!(screen.contains("id"), "screen:\n{screen}");
    assert!(screen.contains("status"), "screen:\n{screen}");
    assert!(screen.contains("created_at"), "screen:\n{screen}");
    // Checkboxes present (both checked and unchecked rows).
    assert!(screen.contains("[x]"), "screen:\n{screen}");
    assert!(screen.contains("[ ]"), "screen:\n{screen}");
    // Type badges (right-aligned) present.
    assert!(screen.contains("int"), "screen:\n{screen}");
    assert!(screen.contains("txt"), "screen:\n{screen}");
    assert!(screen.contains("date"), "screen:\n{screen}");
}

#[test]
fn popup_title_reads_columns() {
    let state = PaletteState::new(cols());
    let screen = render(&state, 40, 8, Rect::new(0, 0, 30, 6));
    assert!(screen.contains("columns"), "screen:\n{screen}");
}

#[test]
fn popup_bottom_border_hints_render() {
    // The popup's bottom border carries ONLY the non-obvious bulk operations. Space/Tab-toggle,
    // ↑↓-nav, and Enter/Esc-close are intuitive and deliberately omitted.
    let state = PaletteState::new(cols());
    let screen = render(&state, 80, 8, Rect::new(0, 0, 80, 6));
    // At 80 cols every bulk-op hint fits.
    assert!(screen.contains("Ctrl+A"), "select-all hint:\n{screen}");
    assert!(screen.contains("Ctrl+X"), "deselect-all hint:\n{screen}");
    assert!(screen.contains("Ctrl+I"), "invert hint:\n{screen}");
    assert!(screen.contains("invert"), "invert description:\n{screen}");
    // The intuitive keys are gone.
    assert!(!screen.contains("toggle"), "no toggle hint:\n{screen}");
    assert!(!screen.contains("nav"), "no nav hint:\n{screen}");
}

#[test]
fn hint_line_width_sums_the_three_bulk_ops() {
    // The popup floors its width to this so the bulk-op hints always fit. The exact value is the
    // rendered width of " Ctrl+A all • Ctrl+X none • Ctrl+I invert".
    let w = hint_line_width();
    // 3 hints: " Ctrl+A all" (11) + " • Ctrl+X none" (14) + " • Ctrl+I invert" (16) = 41.
    assert_eq!(w, 41, "full bulk-op hint line width");
}

#[test]
fn snapshot_palette_80x24() {
    // The headless 80x24 snapshot — a populated palette with a mixed selection at canonical
    // terminal size, including the new bottom-border hint line.
    let mut state = PaletteState::new(cols());
    state.toggle(1); // status checked
    state.toggle(3); // created_at checked
    state.cursor_down(); // cursor on `status`
    let screen = render(&state, 80, 24, Rect::new(0, 1, 60, 8));
    insta::assert_snapshot!(screen);
}

#[test]
fn render_does_not_panic_on_degenerate_area() {
    let state = PaletteState::new(cols());
    for (w, h) in [(1u16, 1u16), (2, 2), (3, 1), (1, 3)] {
        let _ = render(&state, w.max(1), h.max(1), Rect::new(0, 0, w, h));
    }
}

#[test]
fn render_no_op_on_zero_area() {
    let state = PaletteState::new(cols());
    let screen = render(&state, 10, 3, Rect::new(0, 0, 0, 0));
    assert!(
        screen.chars().all(|c| c == ' ' || c == '\n' || c == '"'),
        "zero area must paint nothing, got:\n{screen}"
    );
}

// --- bottom-border hint helper ---

#[test]
fn hint_spans_drops_trailing_hints_on_a_narrow_box() {
    // A 16-char-wide popup can't fit every bulk-op hint; the trailing low-priority ones are
    // dropped whole rather than overflowing the border. (The popup normally floors its width to
    // `hint_line_width` so all three fit — this exercises the truncation path directly.)
    let spans = hint_spans(16);
    let rendered: String = spans.iter().map(|s| s.content.as_ref()).collect();
    // The most-important hint survives (Ctrl+A all).
    assert!(rendered.contains("all"), "got: {rendered:?}");
    // A low-priority hint is dropped.
    assert!(
        !rendered.contains("invert"),
        "trailing hint dropped on narrow popup: {rendered:?}"
    );
}

#[test]
fn hint_spans_zero_width_yields_empty() {
    let spans = hint_spans(0);
    assert!(spans.is_empty());
}

// --- DIAGNOSTIC: dim row bleed-through (user-reported screenshot bug) ---

/// Pin the fix for the screenshot bug where `[x] quantity` showed dim while neighbors were
/// bright. Root cause: the popup is overlaid on top of a results grid whose body cells may carry
/// `Modifier::DIM` (jiq-style stale-error rendering, or NULL-glyph spans from `style_body_line`),
/// and the popup's `Block` does NOT explicitly fill the inner area with a base background style —
/// so any `Span::raw`/empty-cell paint inherits the residual modifiers from the underlying buffer.
/// The check below pre-paints DIM cells via a first draw, then renders the popup over them and
/// asserts no cell inside the popup's inner content area carries `Modifier::DIM`.
#[test]
fn popup_overwrites_underlying_dim_cells() {
    use ratatui::style::{Modifier, Style};
    use ratatui::widgets::Paragraph;

    let mut state = PaletteState::new(cols());
    state.toggle(0);
    state.toggle(2);

    // Pre-paint a DIM background by drawing a Paragraph styled DIM over the full area. Then
    // draw the popup OVER it (on top) and inspect the resulting buffer cells.
    let area = Rect::new(0, 0, 30, 12);
    let mut t = Terminal::new(TestBackend::new(30, 12)).expect("TestBackend");
    t.draw(|f| {
        // Cover the full area with DIM-styled text simulating the underlying stale grid.
        let dim_bg = Paragraph::new("##############################\n".repeat(12))
            .style(Style::default().add_modifier(Modifier::DIM));
        f.render_widget(dim_bg, area);
        // Now render the popup on top of it.
        render_palette(&state, f, area, None);
    })
    .expect("draw");
    let painted = t.backend().buffer();

    // Inner area is the popup minus its border (1-cell on each side).
    let inner = Rect::new(1, 1, area.width - 2, area.height - 2);
    let mut dim_cells: Vec<(u16, u16, String)> = Vec::new();
    for y in inner.y..inner.y + inner.height {
        for x in inner.x..inner.x + inner.width {
            let cell = &painted[(x, y)];
            if cell.modifier.contains(Modifier::DIM) {
                dim_cells.push((x, y, cell.symbol().to_string()));
            }
        }
    }
    assert!(
        dim_cells.is_empty(),
        "popup left {} inner cells with DIM bleeding from underneath: {:?}",
        dim_cells.len(),
        dim_cells
    );
}

#[test]
fn hovered_row_carries_the_hover_band() {
    let state = PaletteState::new(cols()); // cursor on row 0
    let area = Rect::new(0, 0, 40, 6);
    let mut t = Terminal::new(TestBackend::new(50, 10)).expect("TestBackend");
    t.draw(|f| render_palette(&state, f, area, Some(1)))
        .expect("draw");
    let buf = t.backend().buffer();
    assert_eq!(
        buf[(1, 2)].style().bg,
        crate::theme::palette::hovered_bg().bg,
        "hovered (non-cursor) row carries the hover band"
    );
    assert_ne!(
        buf[(1, 3)].style().bg,
        crate::theme::palette::hovered_bg().bg,
        "other rows keep the plain background"
    );
}
