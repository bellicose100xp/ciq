//! Centralized colors and styles (`dev/PLAN.md` §6.8 + project `CLAUDE.md` theme rule).
//!
//! Every color/style ciq paints lives here. The base palette ([`base`]) is a single source of
//! truth in two polarities — the bright **galaxy dark** values verbatim from
//! `jiq/src/theme/galaxy.rs` and a deepened light variant for white terminals — and every semantic
//! accessor (`grid::header()`, `palette::border()`, `app::status_error()`, …) reads whichever one
//! [`base::pal`] says is active. The shell picks the polarity once at startup ([`polarity`]).
//!
//! Render files use `theme::<surface>::<accessor>()` and never import `ratatui::style::Color` or
//! hardcode `Color::*` directly. Borders are focus-aware: focused panes light up with the bright
//! cyan accent; unfocused panes stay quiet in the muted slate.

pub mod base;
pub mod polarity;

#[cfg(test)]
#[path = "theme/base_tests.rs"]
mod base_tests;
#[cfg(test)]
#[path = "theme/polarity_tests.rs"]
mod polarity_tests;
#[cfg(test)]
#[path = "theme/theme_tests.rs"]
mod theme_tests;

pub use polarity::Polarity;

/// App shell colors and styles (query bar, status line, prompts).
pub mod app {
    use super::base as p;
    use ratatui::style::{Modifier, Style};

    pub fn prompt() -> Style {
        Style::default()
            .fg(p::pal().cyan)
            .add_modifier(Modifier::BOLD)
    }

    pub fn query_text() -> Style {
        Style::default().fg(p::pal().text)
    }

    /// The query-bar cursor cell in **Insert** mode — plain reverse-video so the block cursor is
    /// visible regardless of theme polarity (and lands as a styled cell in `TestBackend`).
    pub fn cursor() -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }

    /// The "no cursor" style applied to UNFOCUSED Simple-mode panes so only the focused pane
    /// shows a cursor cell (otherwise tui-textarea paints a reverse-video cursor on every pane it
    /// renders). Plain default style: nothing reversed, nothing colored — text reads as text.
    pub fn cursor_suppressed() -> Style {
        Style::default()
    }

    /// The query-bar cursor cell in vim **Normal** mode — colored reverse-video so the mode reads
    /// at the cursor itself (vim convention).
    pub fn cursor_normal() -> Style {
        Style::default()
            .fg(p::pal().yellow)
            .add_modifier(Modifier::REVERSED)
    }

    /// Default styling for any vim mode badge not covered by [`mode_insert`], [`mode_normal`],
    /// [`mode_operator`], or [`mode_char_search`].
    pub fn mode_indicator() -> Style {
        Style::default()
            .fg(p::pal().yellow)
            .add_modifier(Modifier::BOLD)
    }

    pub fn mode_insert() -> Style {
        Style::default()
            .fg(p::pal().cyan)
            .add_modifier(Modifier::BOLD)
    }

    pub fn mode_normal() -> Style {
        Style::default()
            .fg(p::pal().yellow)
            .add_modifier(Modifier::BOLD)
    }

    pub fn mode_operator() -> Style {
        Style::default()
            .fg(p::pal().green)
            .add_modifier(Modifier::BOLD)
    }

    pub fn mode_char_search() -> Style {
        Style::default()
            .fg(p::pal().pink)
            .add_modifier(Modifier::BOLD)
    }

    pub fn status() -> Style {
        Style::default().fg(p::pal().text_muted)
    }

    pub fn status_error() -> Style {
        Style::default()
            .fg(p::pal().error)
            .add_modifier(Modifier::BOLD)
    }

    pub fn loading() -> Style {
        Style::default()
            .fg(p::pal().yellow)
            .add_modifier(Modifier::DIM)
    }

    /// Kept for compat — banner row is gone, but this style is still referenced. Used by no live
    /// surface; safe to leave in place to avoid a cascade of unrelated edits.
    pub fn truncation_banner() -> Style {
        Style::default()
            .fg(p::pal().yellow)
            .add_modifier(Modifier::BOLD)
    }

    pub fn empty_state() -> Style {
        Style::default().fg(p::pal().text_muted)
    }

    pub fn dialect_summary() -> Style {
        Style::default()
            .fg(p::pal().text_muted)
            .add_modifier(Modifier::DIM)
    }

    /// The fixed-width label column on a Simple-mode pane row (`SELECT`/`WHERE`/`GROUP BY`/
    /// `ORDER BY`/`LIMIT`) when that pane is **unfocused** — muted text so the eye drops to the
    /// editable text on the right.
    pub fn pane_label() -> Style {
        Style::default().fg(p::pal().text_muted)
    }

    /// The Simple-mode pane label when that pane is **focused** — bright cyan + bold so the
    /// focused pane reads at a glance, matching the box's focus-aware border accent.
    pub fn pane_label_focused() -> Style {
        Style::default()
            .fg(p::pal().cyan)
            .add_modifier(Modifier::BOLD)
    }

    /// The faint, **mode-tinted** background band behind the focused Simple-mode pane row. Rather
    /// than a flat neutral fill, it's the mode accent (cyan Insert / yellow Normal / …) blended
    /// ~18% toward the deep base — gentle elevation that stays cohesive with the box's mode-aware
    /// border. `accent` is the focused mode color (or the error red when the query failed).
    pub fn active_pane_bg(accent: ratatui::style::Color) -> Style {
        Style::default().bg(p::tint_bg(accent, 18))
    }

    /// The bright left **accent bar** glyph (`▌`) marking the focused Simple-mode pane — drawn in
    /// the mode accent over the tinted band. A crisp left edge, lazygit/gitui-style, so the active
    /// line reads instantly without a loud full-width fill.
    pub fn active_pane_bar(accent: ratatui::style::Color) -> Style {
        Style::default().fg(accent).bg(p::tint_bg(accent, 18))
    }
}

/// Border styles, **state-aware** (`dev/PLAN.md` §3.1 felt-loop polish).
///
/// jiq-style borders react to the box's current state:
///  - **Query box** border tracks the focused vim mode (Insert=cyan, Normal=yellow,
///    Operator/TextObject=green, CharSearch=pink), with a query-pipeline error overriding the mode
///    color (red). Unfocused boxes recede in muted slate regardless of mode/state.
///  - **Results box** border tracks the displayed result state (Ok=green, Empty=yellow, Error=red,
///    Pending=cyan). Unfocused dims the same hue (no bold) so the state still reads.
pub mod border {
    use super::base as p;
    use crate::app::editor::EditorMode;
    use ratatui::style::{Color, Modifier, Style};

    /// Where the displayed result currently sits in the success/error spectrum — drives the results
    /// box's border color so the user sees the verdict at a glance (jiq's `result_ok` /
    /// `result_warning` / `result_error` / `result_pending` semantics).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ResultState {
        /// No result yet (loading or pre-first-query) — cyan.
        Pending,
        /// Successful query, at least one row — green.
        Ok,
        /// Successful query with zero rows — yellow (no error, but visually distinct so the user
        /// notices the empty result).
        Empty,
        /// Last query failed (preprocess-reject or engine error); the prior result is being kept on
        /// screen DIMMED — red.
        Error,
    }

    /// The accent color for a vim mode (Insert=cyan, Normal=yellow, Operator/TextObject=green,
    /// CharSearch=pink). Pure lookup; both [`query_box`] and the mode-badge style consume it so
    /// the border, the badge, and the bottom-hint keys stay in lockstep.
    pub fn mode_color(mode: EditorMode) -> Color {
        match mode {
            EditorMode::Insert => p::pal().cyan,
            EditorMode::Normal => p::pal().yellow,
            EditorMode::Operator(_) | EditorMode::TextObject(_, _) => p::pal().green,
            EditorMode::CharSearch(_, _) | EditorMode::OperatorCharSearch(_, _, _, _) => {
                p::pal().pink
            }
        }
    }

    /// The accent color for a [`ResultState`] (Ok=green, Empty=yellow, Error=red, Pending=cyan).
    /// Both [`results`] and the row-counter style consume it so the border, the counter, and the
    /// bottom-hint keys all share the same hue.
    pub fn result_color(state: ResultState) -> Color {
        match state {
            ResultState::Ok => p::pal().green,
            ResultState::Empty => p::pal().yellow,
            ResultState::Error => p::pal().red,
            ResultState::Pending => p::pal().cyan,
        }
    }

    /// The bare accent color for the query box: the vim-mode color, or error red when the query
    /// failed. Used for the focused-pane accent bar + tint band (which only render when the box is
    /// focused, so there's no unfocused branch here). [`query_box`] wraps this into a border Style.
    pub fn query_box_accent(mode: EditorMode, has_error: bool) -> Color {
        if has_error {
            p::pal().border_error
        } else {
            mode_color(mode)
        }
    }

    /// The query box's border style: vim-mode color when focused, with `has_error` overriding to
    /// red; unfocused panes recede in muted slate regardless of mode (the focused-or-not reading is
    /// dominant). [`mode_color`] is the canonical color the matching mode badge + hint keys use.
    pub fn query_box(mode: EditorMode, has_error: bool, focused: bool) -> Style {
        if !focused {
            return Style::default().fg(p::pal().border_unfocused);
        }
        let c = if has_error {
            p::pal().border_error
        } else {
            mode_color(mode)
        };
        Style::default().fg(c)
    }

    /// The results box's border style: state color when focused, the same hue dimmed when not.
    /// Unfocused keeps the hue (so the user can still tell ok/empty/error/pending apart at a
    /// glance) but drops the bold and adds DIM so the focused-or-not reading stays dominant.
    pub fn results(state: ResultState, focused: bool) -> Style {
        let c = result_color(state);
        if focused {
            Style::default().fg(c)
        } else {
            Style::default().fg(c).add_modifier(Modifier::DIM)
        }
    }

    /// The border of the currently-focused pane — bright cyan. Kept for any legacy caller that
    /// hasn't moved to the state-aware variants; the live render path uses [`query_box`] /
    /// [`results`] instead.
    pub fn focused() -> Style {
        Style::default().fg(p::pal().border_focused)
    }

    /// The border of an unfocused pane — muted slate so it recedes. Kept for legacy callers.
    pub fn unfocused() -> Style {
        Style::default().fg(p::pal().border_unfocused)
    }

    /// An error border (kept for future use by syntax-error / load-error surfaces).
    pub fn error() -> Style {
        Style::default().fg(p::pal().border_error)
    }
}

/// The bottom keyboard-shortcut help-bar colors and styles (`dev/PLAN.md` §4.1).
pub mod help_line {
    use super::base as p;
    use ratatui::style::{Color, Modifier, Style};

    /// Default key style — bright cyan + bold. Used by popup-internal hint lines that don't
    /// participate in the state-aware accent harmonization (e.g. autocomplete's own bottom-border
    /// hints). The query-box / results-pane borders use [`key_in`] instead so the keys take the
    /// same color as the surrounding border.
    pub fn key() -> Style {
        key_in(p::pal().cyan)
    }

    /// Key style in the given accent — bold + colored. The query-box and results-pane bottom-
    /// border hint lines pass the matching state color (vim-mode color or result-state color) so
    /// the keys harmonize with the border color (jiq's `border_color` plumbed through to its hint
    /// builder). The description and separator stay neutral so the eye reads keys-then-rest.
    pub fn key_in(c: Color) -> Style {
        Style::default().fg(c).add_modifier(Modifier::BOLD)
    }

    pub fn description() -> Style {
        Style::default().fg(p::pal().text)
    }

    pub fn separator() -> Style {
        Style::default().fg(p::pal().text_muted)
    }
}

/// Autocomplete popup colors and styles.
pub mod autocomplete {
    use super::base as p;
    use ratatui::style::{Color, Style};

    /// The popup's accent (cyan) — border, selection band, and left bar all key off it.
    pub fn accent() -> Color {
        p::pal().cyan
    }

    /// Popup border — bright cyan (popup is focused while open).
    pub fn border() -> Style {
        Style::default().fg(accent()).bg(p::pal().bg)
    }

    /// Non-selected row: normal text on the opaque popup surface.
    pub fn item() -> Style {
        super::popup::item()
    }

    /// Selected row band (accent text on an elevated background + bold) — the modern replacement
    /// for reverse-video, paired with the left accent bar.
    pub fn selected() -> Style {
        super::popup::selected_band(accent())
    }

    /// The row under the mouse pointer — a faint elevation band (below the selection band so hover
    /// never fights the selected row), on the opaque surface.
    pub fn hovered_bg() -> Style {
        super::popup::hovered(p::pal().text)
    }

    pub fn type_hint() -> Style {
        // TEXT_DIM color on the opaque surface (not Modifier::DIM, which bleeds through).
        Style::default().fg(p::pal().text_dim).bg(p::pal().bg)
    }
}

/// Column-palette popup colors and styles. Module name preserved for callers — base palette
/// constants live in [`base`] above to keep this name free for the popup.
pub mod palette {
    use super::base as p;
    use ratatui::style::{Color, Modifier, Style};

    /// Distinct popup accent (magenta) — the SELECT-pane column picker reads as visually separate
    /// from the cyan-default popups (autocomplete, history, AI, facet).
    pub fn accent() -> Color {
        p::pal().magenta
    }

    pub fn border() -> Style {
        Style::default().fg(accent()).bg(p::pal().bg)
    }

    pub fn item() -> Style {
        super::popup::item()
    }

    /// Cursor row: the modern accent band + bold (with the left bar), on the opaque surface.
    pub fn selected() -> Style {
        super::popup::selected_band(accent())
    }

    pub fn checked() -> Style {
        Style::default()
            .fg(p::pal().green)
            .bg(p::pal().bg)
            .add_modifier(Modifier::BOLD)
    }

    /// The row under the mouse pointer — same faint band as the other list popups.
    pub fn hovered_bg() -> Style {
        super::popup::hovered(p::pal().text)
    }

    pub fn type_hint() -> Style {
        // TEXT_DIM color on the opaque surface rather than Modifier::DIM (which bleeds through
        // popup renders via ratatui's style-OR semantics).
        Style::default().fg(p::pal().text_dim).bg(p::pal().bg)
    }

    /// The popup's title text (`" columns "`). Same magenta accent as the border so the title
    /// reads as part of the popup chrome.
    pub fn title() -> Style {
        Style::default()
            .fg(accent())
            .bg(p::pal().bg)
            .add_modifier(Modifier::BOLD)
    }
}

/// Shared popup chrome — the one place the "modern popup" look is defined so every popup
/// (autocomplete, palette, history, facet, AI) reads as one system: an **opaque** surface (so the
/// grid never leaks through), a selected-row band elevated off that surface, and a bright left
/// accent bar (`▌`) on the current row (the grid's lazygit-style bar, brought to the popups).
pub mod popup {
    use super::base as p;
    use ratatui::style::{Color, Modifier, Style};

    /// The glyph for the current-row left accent bar — a three-eighths block (`▍`), a touch
    /// thinner than the half block so the selection's leading edge reads as a crisp stripe, not a
    /// heavy slab.
    pub const BAR: &str = "\u{258d}";

    /// Opaque popup fill — set as the `Block`'s `.style()` so the whole box (border-inner
    /// included) paints over the grid behind it. Paired with a `Clear` first so no stale
    /// modifier (a dimmed/NULL grid cell) bleeds into the popup.
    pub fn surface() -> Style {
        Style::default().bg(p::pal().bg)
    }

    /// A non-selected popup row's base style: normal text on the opaque surface (the explicit bg
    /// keeps a row from inheriting anything underneath).
    pub fn item() -> Style {
        Style::default().fg(p::pal().text).bg(p::pal().bg)
    }

    /// A dimmed/hint popup row (badges, "(no matches)"): muted text on the opaque surface.
    pub fn item_dim() -> Style {
        Style::default().fg(p::pal().text_muted).bg(p::pal().bg)
    }

    /// The row under the mouse pointer — a faint elevation band (below the selection band so hover
    /// never fights the current row), on the opaque surface.
    pub fn hovered(fg: Color) -> Style {
        Style::default().fg(fg).bg(p::pal().bg_hover)
    }

    /// The hover band's background color, for renderers that fold it into each span's `bg` (a
    /// row whose spans set their own opaque bg can't take a line-level hover style).
    pub fn hover_bg() -> Color {
        p::pal().bg_hover
    }

    /// The **selected / cursor** row band: the popup's accent as the text color over an elevated
    /// background, bold — a solid, legible band rather than reverse-video (which inverts whatever
    /// leaked through). Distinct from hover so the two never look alike.
    pub fn selected_band(accent: Color) -> Style {
        Style::default()
            .fg(accent)
            .bg(p::pal().bg_highlight)
            .add_modifier(Modifier::BOLD)
    }

    /// The bright left accent bar (`▌`) on the selected row, in the popup's accent over the same
    /// elevated band so it reads as the leading edge of the selection.
    pub fn selected_bar(accent: Color) -> Style {
        Style::default()
            .fg(accent)
            .bg(p::pal().bg_highlight)
            .add_modifier(Modifier::BOLD)
    }
}

/// Facet popup colors and styles.
pub mod facets {
    use super::base as p;
    use ratatui::style::{Modifier, Style};

    pub fn border() -> Style {
        Style::default().fg(p::pal().border_focused).bg(p::pal().bg)
    }

    pub fn label() -> Style {
        // Muted text on the opaque surface (color, not Modifier::DIM which bleeds through).
        Style::default().fg(p::pal().text_muted).bg(p::pal().bg)
    }

    pub fn value() -> Style {
        Style::default()
            .fg(p::pal().cyan)
            .bg(p::pal().bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn bar() -> Style {
        Style::default().fg(p::pal().cyan).bg(p::pal().bg)
    }

    pub fn hint() -> Style {
        Style::default().fg(p::pal().text_muted).bg(p::pal().bg)
    }
}

/// Query-history popup colors and styles.
pub mod history {
    use super::base as p;
    use ratatui::style::{Color, Style};

    /// The popup's accent (cyan) — border, selection band, and left bar all key off it.
    pub fn accent() -> Color {
        p::pal().border_focused
    }

    pub fn border() -> Style {
        Style::default().fg(accent()).bg(p::pal().bg)
    }

    pub fn item() -> Style {
        super::popup::item()
    }

    /// Cursor row: the modern accent band + bold (with the left bar), on the opaque surface.
    pub fn selected() -> Style {
        super::popup::selected_band(accent())
    }

    /// The row under the mouse pointer — same faint band as the other list popups.
    pub fn hovered_bg() -> Style {
        super::popup::hovered(p::pal().text)
    }

    pub fn hint() -> Style {
        // Muted text on the opaque surface (color-based, not Modifier::DIM which bleeds through).
        Style::default().fg(p::pal().text_muted).bg(p::pal().bg)
    }
}

/// AI NL->SQL popup colors and styles.
pub mod ai {
    use super::base as p;
    use ratatui::style::{Modifier, Style};

    /// AI popup border — purple to mark it as the AI surface (still bright, distinct from the
    /// other focused popups).
    pub fn border() -> Style {
        Style::default().fg(p::pal().purple).bg(p::pal().bg)
    }

    pub fn input() -> Style {
        Style::default().fg(p::pal().text).bg(p::pal().bg)
    }

    pub fn pending() -> Style {
        // Color-muted on the opaque surface (Modifier::DIM bleeds through popup renders).
        Style::default().fg(p::pal().text_muted).bg(p::pal().bg)
    }

    pub fn success() -> Style {
        Style::default()
            .fg(p::pal().green)
            .bg(p::pal().bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn error() -> Style {
        Style::default()
            .fg(p::pal().error)
            .bg(p::pal().bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn hint() -> Style {
        Style::default().fg(p::pal().text_muted).bg(p::pal().bg)
    }
}

/// Save-to-CSV popup (`Ctrl+W`) colors and styles. Green accent — the "write it out" verb reads
/// as a success-colored surface, distinct from the cyan (autocomplete/history/facet), magenta
/// (palette), and purple (AI) popups.
pub mod save {
    use super::base as p;
    use ratatui::style::{Color, Modifier, Style};

    /// The popup's accent (green) — border and title key off it.
    pub fn accent() -> Color {
        p::pal().green
    }

    pub fn border() -> Style {
        Style::default().fg(accent()).bg(p::pal().bg)
    }

    /// The popup's title text, same accent as the border so the chrome reads as one.
    pub fn title() -> Style {
        Style::default()
            .fg(accent())
            .bg(p::pal().bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn prompt() -> Style {
        Style::default()
            .fg(accent())
            .bg(p::pal().bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn input() -> Style {
        Style::default().fg(p::pal().text).bg(p::pal().bg)
    }

    /// The block-cursor cell after the filename (reverse-video, like the query bar's Insert
    /// cursor) so "typing goes here" reads at a glance.
    pub fn cursor() -> Style {
        Style::default()
            .bg(p::pal().bg)
            .add_modifier(Modifier::REVERSED)
    }

    /// The resolved-path preview line (`→ /path/to/out.csv`) — muted on the opaque surface.
    pub fn preview() -> Style {
        Style::default().fg(p::pal().text_muted).bg(p::pal().bg)
    }

    /// The overwrite warning appended to the preview when the destination already exists.
    pub fn overwrite_warning() -> Style {
        Style::default()
            .fg(p::pal().warning)
            .bg(p::pal().bg)
            .add_modifier(Modifier::BOLD)
    }

    /// An inline resolve/write error — the popup stays open so the user can fix and retry.
    pub fn error() -> Style {
        Style::default()
            .fg(p::pal().error)
            .bg(p::pal().bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn hint() -> Style {
        Style::default().fg(p::pal().text_muted).bg(p::pal().bg)
    }
}

/// Search bar (`Ctrl+F` row filter) colors and styles. Polarity follows jiq's search bar: the
/// pink accent while editing (active), the muted slate once confirmed (the filter is frozen and
/// keys navigate the grid again).
pub mod search {
    use super::base as p;
    use ratatui::style::{Modifier, Style};

    pub fn border_active() -> Style {
        Style::default().fg(p::pal().pink)
    }

    pub fn border_inactive() -> Style {
        Style::default().fg(p::pal().border_unfocused)
    }

    pub fn text_active() -> Style {
        Style::default().fg(p::pal().text)
    }

    pub fn text_inactive() -> Style {
        Style::default().fg(p::pal().text_muted)
    }

    /// The `shown/total` filtered-row badge on the bar's top border.
    pub fn badge() -> Style {
        Style::default().fg(p::pal().cyan)
    }

    /// The badge when the needle matched zero rows.
    pub fn badge_no_matches() -> Style {
        Style::default()
            .fg(p::pal().error)
            .add_modifier(Modifier::BOLD)
    }

    pub fn hint() -> Style {
        Style::default()
            .fg(p::pal().text_muted)
            .add_modifier(Modifier::DIM)
    }
}

/// Grid (results table) colors and styles.
pub mod grid {
    use super::base as p;
    use ratatui::style::{Color, Modifier, Style};

    /// Sticky header row — bright cyan + bold. Kept as the neutral fallback; the live header is
    /// painted per-column by [`header_column`] so each label carries its column's pastel hue.
    pub fn header() -> Style {
        Style::default()
            .fg(p::pal().cyan)
            .add_modifier(Modifier::BOLD)
    }

    /// The pastel hue for the column at absolute index `col_index`, rotating through
    /// [`COLUMN_PASTELS`](p::pal().column_pastels). Keyed on the absolute (not visible) index so a column
    /// keeps its color as the grid scrolls horizontally. Pure and branch-free (a modulo index into
    /// a const table), so painting every visible cell with it stays O(1) per cell.
    fn column_hue(col_index: usize) -> Color {
        p::pal().column_pastels[col_index % p::pal().column_pastels.len()]
    }

    /// A body cell in the column at absolute index `col_index` — its pastel hue, so adjacent
    /// columns read as distinct bands when scanning a wide result.
    pub fn column(col_index: usize) -> Style {
        Style::default().fg(column_hue(col_index))
    }

    /// The sticky-header label for the column at absolute index `col_index` — the same pastel hue
    /// as its body cells, bolded so the header still reads as a header. Ties each header to the
    /// color of the data below it.
    pub fn header_column(col_index: usize) -> Style {
        Style::default()
            .fg(column_hue(col_index))
            .add_modifier(Modifier::BOLD)
    }

    /// Neutral body-cell style — the fallback when a cell has no column context (e.g. a
    /// pre-built [`BodyRow`](crate::grid::grid_layout::BodyRow) test fixture). Live rendering uses
    /// [`column`] so cells carry their column's pastel hue.
    pub fn cell() -> Style {
        Style::default().fg(p::pal().text)
    }

    /// A SQL `NULL` cell's glyph — muted + dim so a null reads as "absent value".
    pub fn null() -> Style {
        Style::default()
            .fg(p::pal().text_muted)
            .add_modifier(Modifier::DIM)
    }

    pub fn stale_modifier() -> Modifier {
        Modifier::DIM
    }

    /// Background band for the grid body row under the mouse pointer. Brighter than jiq's hover
    /// (which sits just above the base) — ciq wants the hovered row to read clearly, so this uses
    /// the top of the background ramp ([`BG_HIGHLIGHT`](p::pal().bg_highlight)).
    pub fn hovered_bg() -> Style {
        Style::default().bg(p::pal().bg_highlight)
    }

    /// The bright left accent bar (`▌`) that rides column 0 of the hovered row and follows the
    /// pointer (lazygit-style, matching ciq's focused-pane bar). Painted in the pane's state
    /// accent so it harmonizes with the border and row counter.
    pub fn hover_bar(accent: Color) -> Style {
        Style::default()
            .fg(accent)
            .bg(p::pal().bg_highlight)
            .add_modifier(Modifier::BOLD)
    }

    /// A `Ctrl+F` search-match run within a body line — jiq's match-highlight band ([`BG_MATCH`],
    /// brighter than the hover band) with bright text so a match reads clearly in place without
    /// drowning the row. This is the **non-current** match style; the actively-selected match uses
    /// [`current_match`].
    pub fn search_match() -> Style {
        Style::default()
            .fg(p::pal().text)
            .bg(p::pal().bg_match)
            .add_modifier(Modifier::BOLD)
    }

    /// The **current** search match (the one `n`/`N` navigation lands on) — jiq's bright orange
    /// band with a dark fg for maximum contrast, so the active result stands out from the other
    /// (dim-slate) matches on screen.
    pub fn current_match() -> Style {
        Style::default()
            .fg(p::pal().bg)
            .bg(p::pal().orange)
            .add_modifier(Modifier::BOLD)
    }
}

/// Results pane chrome (border, row counter).
pub mod results {
    use super::base as p;
    use ratatui::style::{Color, Modifier, Style};

    /// Default row-counter style — bright cyan. Kept for callers that don't yet route through the
    /// state-aware [`row_counter_in`].
    pub fn row_counter() -> Style {
        row_counter_in(p::pal().cyan)
    }

    /// `<rendered>/<total>` row counter in the given accent — matching the results-box border so
    /// the counter reads as one with the chrome (jiq's per-state accent on the results scrollbar /
    /// counter).
    pub fn row_counter_in(c: Color) -> Style {
        Style::default().fg(c)
    }

    /// Row counter while the displayed result is stale — muted + dim to ride the stale-grid
    /// polarity.
    pub fn row_counter_stale() -> Style {
        Style::default()
            .fg(p::pal().text_muted)
            .add_modifier(Modifier::DIM)
    }
}
