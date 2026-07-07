//! The bottom keyboard-shortcut help bar (`dev/PLAN.md` §4.1) — a context-sensitive legend on the
//! very bottom row, ported from jiq's `src/help/help_line_render.rs` and re-justified on ciq's
//! merits: ciq's chords are its own (SQL palette / facet / history / AI / vim), not jq's
//! (snippets / save / output-query), so the hint sets are rebuilt from ciq's real key handlers
//! ([`App::on_key`](super::App::on_key) and the per-popup handlers).
//!
//! Two pieces, split by purity:
//!  - [`get_context_hints`] is a **pure function of `App` state** -> the ordered `(key, desc)` hints
//!    for the current focus / vim mode / open popup. Most-important hints come first, so a narrow
//!    terminal drops the *trailing* (lowest-priority) hints rather than overflowing. It sits on the
//!    pure-core hard floor (`dev/core-modules.txt`) — one table-test row per context.
//!  - [`render_line`] blits the styled hints onto the help row. A `TestBackend`-snapshot seam like
//!    the other `*_render` surfaces; it never names a `Color` (all styles come from
//!    [`theme::help_line`]). The mode badge no longer rides the help row — it lives on the query
//!    box's TOP border (`app_render::render_query_box`), so this layer is hint-only.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, Focus, QueryMode};
use crate::theme;

/// Build the ordered `(key, desc)` hint list — like jiq's `hints!` macro, a terse literal table.
macro_rules! hints {
    ($($key:literal => $desc:literal),+ $(,)?) => {
        vec![$(($key, $desc)),+]
    };
}

/// The context-sensitive shortcut hints for the current `App` state, most-important first.
///
/// The branch order mirrors [`App::on_key`](super::App::on_key)'s routing precedence: an open popup
/// intercepts keys first (AI -> history -> palette -> facet -> autocomplete), then the focused
/// surface (results pane vs query bar), with the query bar split by vim mode. Pure: no `Frame`, no
/// time, no I/O — just `App` state in, hints out (the hard-floor contract).
pub fn get_context_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    // Hint sets carry ONLY non-obvious chords. Intuitive keys (Enter, Esc, Tab, arrows, PgUp/PgDn,
    // Home, Space, type-to-filter) are omitted — the legend teaches what a user can't guess, not
    // what they already know. `Ctrl+C quit` is the one universal we keep, since "how do I get out"
    // is not always obvious in a full-screen TUI.
    if app.is_ai_open() {
        return hints!["Ctrl+C" => "quit"];
    }
    if app.is_history_open() {
        return hints!["Ctrl+C" => "quit"];
    }
    if app.is_palette_open() {
        // The popup carries its own bottom-border hint line (the Ctrl+A/X/I bulk ops); the main
        // bottom-border just shows quit in case it is ever consulted outside the popup's chrome.
        return hints![
            "Ctrl+A" => "all",
            "Ctrl+X" => "none",
            "Ctrl+I" => "invert",
            "Ctrl+C" => "quit",
        ];
    }
    if app.is_facet_open() {
        return hints!["Ctrl+C" => "quit"];
    }
    // The search bar's editing mode captures the keyboard; its own bottom border teaches
    // Enter/Esc, so the main legend just shows quit (like the other capture surfaces).
    if app.search().is_editing() {
        return hints!["Ctrl+C" => "quit"];
    }
    if app.autocomplete().is_open() {
        return hints!["Ctrl+C" => "quit"];
    }
    if app.focus() == Focus::Results {
        // A confirmed search adds the two chords that act on it (re-edit / clear).
        if app.search().is_confirmed() {
            return hints![
                "Ctrl+F" => "edit search",
                "Esc" => "clear search",
                "f" => "facet",
                "Ctrl+T" => "query",
                "Ctrl+C" => "quit",
            ];
        }
        // Arrow/PgUp/PgDn/Home scrolling is intuitive — only the non-obvious chords show.
        return hints![
            "f" => "facet",
            "Ctrl+F" => "search",
            "Ctrl+T" => "query",
            "Ctrl+C" => "quit",
        ];
    }
    // Query bar focused. Insert mode is the typing path (live query); the vim command modes share a
    // motion-oriented hint set. With the autocomplete popup CLOSED, the Simple-mode bar uses
    // `Alt+↑/↓` (or `Alt+J/K`) for pane nav and Tab inserts a literal `\t`; Power mode keeps Tab as
    // autocomplete-complete (or a textarea tab when no popup is up). `Ctrl+T` toggles focus to the
    // results pane.
    if app.editor_mode().is_insert() {
        // Mode badge on the TOP border already announces INSERT/NORMAL (jiq's pattern), so the
        // bottom hints don't repeat `Tab \t` or `Esc vim`. The `Ctrl+P columns` hint only appears
        // when SELECT pane has focus (Simple mode) — Ctrl+P is anchored to that pane.
        let mut hints: Vec<(&'static str, &'static str)> = match app.query_form().mode() {
            QueryMode::Simple => {
                let mut v = vec![("Alt+\u{2191}\u{2193}", "panes")];
                if app.query_form().focused_pane() == crate::app::SimplePane::Select {
                    v.push(("Ctrl+P", "columns"));
                }
                v.extend([
                    ("Ctrl+A", "AI"),
                    ("Ctrl+R", "history"),
                    ("Ctrl+T", "results"),
                    ("Ctrl+Q", "SQL mode"),
                    ("Ctrl+C", "quit"),
                ]);
                v
            }
            QueryMode::Power => vec![
                // Tab-complete is intuitive in an autocomplete context — omitted.
                ("Ctrl+A", "AI"),
                ("Ctrl+R", "history"),
                ("Ctrl+T", "results"),
                ("Ctrl+Q", "SQL mode"),
                ("Ctrl+C", "quit"),
            ],
        };
        hints.shrink_to_fit();
        hints
    } else {
        // Vim Normal mode: hjkl/i are obvious to anyone using vim mode; only the feature chords
        // (and quit) show. The TOP-border badge already announces NORMAL.
        hints![
            "Ctrl+R" => "history",
            "Ctrl+T" => "results",
            "Ctrl+Q" => "SQL mode",
            "Ctrl+C" => "quit",
        ]
    }
}

/// The vim mode badge shown on the query box's TOP border when the query bar is focused (`INSERT` /
/// `NORMAL` / a pending-key hint like `d(`); `None` when the results pane is focused (no editing
/// mode applies there). Pure — a thin projection of [`App::editor_mode`].
pub fn mode_label(app: &App) -> Option<String> {
    if app.focus() == Focus::QueryBar {
        Some(app.editor_mode().display())
    } else {
        None
    }
}

/// Pick the per-mode badge style (Insert / Normal / Operator-pending / CharSearch-pending). Mirrors
/// jiq's per-mode badge color so the mode reads at a glance independent of the chord text.
pub fn mode_badge_style(app: &App) -> ratatui::style::Style {
    use crate::app::editor::EditorMode;
    match app.editor_mode() {
        EditorMode::Insert => theme::app::mode_insert(),
        EditorMode::Normal => theme::app::mode_normal(),
        EditorMode::Operator(_) | EditorMode::TextObject(_, _) => theme::app::mode_operator(),
        EditorMode::CharSearch(_, _) | EditorMode::OperatorCharSearch(_, _, _, _) => {
            theme::app::mode_char_search()
        }
    }
}

/// Build the styled hint spans, dropping the lowest-priority *trailing* hints so the line never
/// overflows `max_width`. Each hint is `key` (accented) + space + `desc` (quiet); the bullet
/// `\u{2022}` between hints is rendered in the separator style. The leading bullet stays even when
/// the line is centered, so the legend reads as one compact unit. Default key color (cyan) — see
/// [`build_styled_spans_in`] for a variant that takes the accent so the keys harmonize with the
/// surrounding border.
fn build_styled_spans(
    hints: &[(&'static str, &'static str)],
    max_width: usize,
) -> Vec<Span<'static>> {
    build_styled_spans_in(hints, max_width, None)
}

/// Build styled hint spans with the **key** color set to `accent` — used by the query-box and
/// results-pane bottom-border hints so the chord names match the box's border accent. `None` falls
/// back to the default cyan key style (used by tests and any popup that doesn't share the box's
/// state-aware accent).
fn build_styled_spans_in(
    hints: &[(&'static str, &'static str)],
    max_width: usize,
    accent: Option<ratatui::style::Color>,
) -> Vec<Span<'static>> {
    let key_style = match accent {
        Some(c) => theme::help_line::key_in(c),
        None => theme::help_line::key(),
    };
    let desc_style = theme::help_line::description();
    let sep_style = theme::help_line::separator();

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(hints.len() * 4);
    let mut width = 0usize;

    for (key, desc) in hints {
        // Each hint after the first content element is preceded by " \u{2022} " (the bullet); the
        // very first content element is preceded by a single leading space. Compute the candidate
        // width before committing so a hint that wouldn't fit is dropped whole (no clipped word).
        let lead_is_first = spans.is_empty();
        let sep = if lead_is_first { " " } else { " \u{2022} " };
        let chunk_w = sep.chars().count() + key.chars().count() + 1 + desc.chars().count();
        if width + chunk_w > max_width {
            break; // this and every lower-priority hint are dropped (narrow-width policy).
        }
        spans.push(Span::styled(sep.to_string(), sep_style));
        spans.push(Span::styled(*key, key_style));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(*desc, desc_style));
        width += chunk_w;
    }

    spans
}

/// The styled help spans for the current context — context hints only, laid out width-aware so the
/// lowest-priority *trailing* hints drop when `max_width` is tight. Pure: `App` state + a width in,
/// styled spans out. The mode badge is no longer included here — it rides the query box's TOP
/// border (`app_render::render_query_box`). Default cyan key color; see [`hint_spans_in`] for the
/// accent-harmonized variant the live render uses.
pub fn hint_spans(app: &App, max_width: usize) -> Vec<Span<'static>> {
    let hints = get_context_hints(app);
    build_styled_spans(&hints, max_width)
}

/// The styled help spans with the key color set to `accent` — what the live query box and results
/// pane render on their respective bottom borders so the chord-key colors harmonize with the
/// box's state-aware border accent (vim-mode color for the query box, result-state color for the
/// results pane).
pub fn hint_spans_in(
    app: &App,
    max_width: usize,
    accent: ratatui::style::Color,
) -> Vec<Span<'static>> {
    let hints = get_context_hints(app);
    build_styled_spans_in(&hints, max_width, Some(accent))
}

/// Render the help hints onto a one-row `area` as a standalone line (kept for direct
/// `TestBackend`-snapshot testing of the hint layout). No-op on a zero-size area. The live app
/// renders the same [`hint_spans`] on the query box's bottom border (centered) instead of calling
/// this — but tests still drive this seam directly.
pub fn render_line(app: &App, frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let spans = hint_spans(app, area.width as usize);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
#[path = "help_line_tests.rs"]
mod help_line_tests;
