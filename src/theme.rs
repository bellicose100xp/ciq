//! Centralized colors and styles (`dev/PLAN.md` §6.8 + project `CLAUDE.md` theme rule).
//!
//! Every color/style ciq paints lives here. The base palette ([`base`]) is a single source of
//! truth — the bright **galaxy dark** values verbatim from `jiq/src/theme/galaxy.rs` (electric cyan
//! / golden yellow / fresh green / soft magenta + pink on a deep-space-blue background) — and every
//! semantic accessor (`grid::header()`, `palette::border()`, `app::status_error()`, …) maps onto it.
//!
//! Render files use `theme::<surface>::<accessor>()` and never import `ratatui::style::Color` or
//! hardcode `Color::*` directly. Borders are focus-aware: focused panes light up with the bright
//! cyan accent; unfocused panes stay quiet in the muted slate.

/// The ciq base palette — verbatim from `jiq/src/theme/galaxy.rs::galaxy_dark`. One source of truth
/// every semantic accessor maps onto.
pub mod base {
    use ratatui::style::Color;

    // --- text + background ramp ---
    pub const TEXT: Color = Color::Rgb(236, 236, 244);
    pub const TEXT_DIM: Color = Color::Rgb(90, 92, 119);
    pub const TEXT_MUTED: Color = Color::Rgb(130, 133, 158);
    pub const BG_DARK: Color = Color::Rgb(26, 26, 46);
    pub const BG_SURFACE: Color = Color::Rgb(35, 35, 58);
    pub const BG_HOVER: Color = Color::Rgb(45, 45, 72);
    pub const BG_HIGHLIGHT: Color = Color::Rgb(55, 55, 85);

    // --- accents ---
    pub const CYAN: Color = Color::Rgb(0, 217, 255);
    pub const YELLOW: Color = Color::Rgb(255, 217, 61);
    pub const GREEN: Color = Color::Rgb(107, 203, 119);
    pub const MAGENTA: Color = Color::Rgb(198, 120, 221);
    pub const PINK: Color = Color::Rgb(255, 107, 157);
    pub const RED: Color = Color::Rgb(224, 108, 117);
    pub const ORANGE: Color = Color::Rgb(255, 184, 108);
    pub const PURPLE: Color = Color::Rgb(189, 147, 249);

    // --- semantic aliases ---
    pub const SUCCESS: Color = GREEN;
    pub const WARNING: Color = YELLOW;
    pub const ERROR: Color = RED;
    pub const INFO: Color = CYAN;

    // --- borders ---
    pub const BORDER_FOCUSED: Color = CYAN;
    pub const BORDER_UNFOCUSED: Color = TEXT_DIM;
    pub const BORDER_ERROR: Color = RED;
    pub const BORDER_WARNING: Color = YELLOW;

    /// Blend `accent` toward [`BG_DARK`] by `accent_pct`/100, yielding a faint accent-tinted
    /// background (no alpha in a terminal, so we mix RGB toward the base). Used for the focused
    /// query-pane elevation band — a low percentage reads as "the base, gently tinted by the mode
    /// color" rather than a loud fill. Falls back to `BG_SURFACE` if either color isn't RGB.
    pub fn tint_bg(accent: Color, accent_pct: u16) -> Color {
        fn mix(a: u8, b: u8, pct: u16) -> u8 {
            ((a as u16 * pct + b as u16 * (100 - pct)) / 100) as u8
        }
        match (accent, BG_DARK) {
            (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => Color::Rgb(
                mix(ar, br, accent_pct),
                mix(ag, bg, accent_pct),
                mix(ab, bb, accent_pct),
            ),
            _ => BG_SURFACE,
        }
    }
}

/// App shell colors and styles (query bar, status line, prompts).
pub mod app {
    use super::base as p;
    use ratatui::style::{Modifier, Style};

    pub fn prompt() -> Style {
        Style::default().fg(p::CYAN).add_modifier(Modifier::BOLD)
    }

    pub fn query_text() -> Style {
        Style::default().fg(p::TEXT)
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
            .fg(p::YELLOW)
            .add_modifier(Modifier::REVERSED)
    }

    /// Default styling for any vim mode badge not covered by [`mode_insert`], [`mode_normal`],
    /// [`mode_operator`], or [`mode_char_search`].
    pub fn mode_indicator() -> Style {
        Style::default().fg(p::YELLOW).add_modifier(Modifier::BOLD)
    }

    pub fn mode_insert() -> Style {
        Style::default().fg(p::CYAN).add_modifier(Modifier::BOLD)
    }

    pub fn mode_normal() -> Style {
        Style::default().fg(p::YELLOW).add_modifier(Modifier::BOLD)
    }

    pub fn mode_operator() -> Style {
        Style::default().fg(p::GREEN).add_modifier(Modifier::BOLD)
    }

    pub fn mode_char_search() -> Style {
        Style::default().fg(p::PINK).add_modifier(Modifier::BOLD)
    }

    pub fn status() -> Style {
        Style::default().fg(p::TEXT_MUTED)
    }

    pub fn status_error() -> Style {
        Style::default().fg(p::ERROR).add_modifier(Modifier::BOLD)
    }

    pub fn loading() -> Style {
        Style::default().fg(p::YELLOW).add_modifier(Modifier::DIM)
    }

    /// Kept for compat — banner row is gone, but this style is still referenced. Used by no live
    /// surface; safe to leave in place to avoid a cascade of unrelated edits.
    pub fn truncation_banner() -> Style {
        Style::default().fg(p::YELLOW).add_modifier(Modifier::BOLD)
    }

    pub fn empty_state() -> Style {
        Style::default().fg(p::TEXT_MUTED)
    }

    pub fn dialect_summary() -> Style {
        Style::default()
            .fg(p::TEXT_MUTED)
            .add_modifier(Modifier::DIM)
    }

    /// The fixed-width label column on a Simple-mode pane row (`SELECT`/`WHERE`/`GROUP BY`/
    /// `ORDER BY`/`LIMIT`) when that pane is **unfocused** — muted text so the eye drops to the
    /// editable text on the right.
    pub fn pane_label() -> Style {
        Style::default().fg(p::TEXT_MUTED)
    }

    /// The Simple-mode pane label when that pane is **focused** — bright cyan + bold so the
    /// focused pane reads at a glance, matching the box's focus-aware border accent.
    pub fn pane_label_focused() -> Style {
        Style::default().fg(p::CYAN).add_modifier(Modifier::BOLD)
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
            EditorMode::Insert => p::CYAN,
            EditorMode::Normal => p::YELLOW,
            EditorMode::Operator(_) | EditorMode::TextObject(_, _) => p::GREEN,
            EditorMode::CharSearch(_, _) | EditorMode::OperatorCharSearch(_, _, _, _) => p::PINK,
        }
    }

    /// The accent color for a [`ResultState`] (Ok=green, Empty=yellow, Error=red, Pending=cyan).
    /// Both [`results`] and the row-counter style consume it so the border, the counter, and the
    /// bottom-hint keys all share the same hue.
    pub fn result_color(state: ResultState) -> Color {
        match state {
            ResultState::Ok => p::GREEN,
            ResultState::Empty => p::YELLOW,
            ResultState::Error => p::RED,
            ResultState::Pending => p::CYAN,
        }
    }

    /// The bare accent color for the query box: the vim-mode color, or error red when the query
    /// failed. Used for the focused-pane accent bar + tint band (which only render when the box is
    /// focused, so there's no unfocused branch here). [`query_box`] wraps this into a border Style.
    pub fn query_box_accent(mode: EditorMode, has_error: bool) -> Color {
        if has_error {
            p::BORDER_ERROR
        } else {
            mode_color(mode)
        }
    }

    /// The query box's border style: vim-mode color when focused, with `has_error` overriding to
    /// red; unfocused panes recede in muted slate regardless of mode (the focused-or-not reading is
    /// dominant). [`mode_color`] is the canonical color the matching mode badge + hint keys use.
    pub fn query_box(mode: EditorMode, has_error: bool, focused: bool) -> Style {
        if !focused {
            return Style::default().fg(p::BORDER_UNFOCUSED);
        }
        let c = if has_error {
            p::BORDER_ERROR
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
        Style::default().fg(p::BORDER_FOCUSED)
    }

    /// The border of an unfocused pane — muted slate so it recedes. Kept for legacy callers.
    pub fn unfocused() -> Style {
        Style::default().fg(p::BORDER_UNFOCUSED)
    }

    /// An error border (kept for future use by syntax-error / load-error surfaces).
    pub fn error() -> Style {
        Style::default().fg(p::BORDER_ERROR)
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
        key_in(p::CYAN)
    }

    /// Key style in the given accent — bold + colored. The query-box and results-pane bottom-
    /// border hint lines pass the matching state color (vim-mode color or result-state color) so
    /// the keys harmonize with the border color (jiq's `border_color` plumbed through to its hint
    /// builder). The description and separator stay neutral so the eye reads keys-then-rest.
    pub fn key_in(c: Color) -> Style {
        Style::default().fg(c).add_modifier(Modifier::BOLD)
    }

    pub fn description() -> Style {
        Style::default().fg(p::TEXT)
    }

    pub fn separator() -> Style {
        Style::default().fg(p::TEXT_MUTED)
    }
}

/// Autocomplete popup colors and styles.
pub mod autocomplete {
    use super::base as p;
    use ratatui::style::{Modifier, Style};

    /// Popup border — bright cyan (popup is focused while open).
    pub fn border() -> Style {
        Style::default().fg(p::BORDER_FOCUSED)
    }

    pub fn item() -> Style {
        Style::default().fg(p::TEXT)
    }

    pub fn selected() -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }

    pub fn type_hint() -> Style {
        // TEXT_DIM color, not Modifier::DIM — same reasoning as palette::type_hint.
        Style::default().fg(p::TEXT_DIM)
    }
}

/// Column-palette popup colors and styles. Module name preserved for callers — base palette
/// constants live in [`base`] above to keep this name free for the popup.
pub mod palette {
    use super::base as p;
    use ratatui::style::{Modifier, Style};

    /// Distinct popup accent (magenta) — the SELECT-pane column picker reads as visually separate
    /// from the cyan-default popups (autocomplete, history, AI, facet).
    pub fn border() -> Style {
        Style::default().fg(p::MAGENTA)
    }

    pub fn item() -> Style {
        Style::default().fg(p::TEXT)
    }

    /// Cursor row: reverse-video over the magenta accent so the highlight reads even on a busy
    /// background.
    pub fn selected() -> Style {
        Style::default()
            .fg(p::MAGENTA)
            .add_modifier(Modifier::REVERSED)
    }

    pub fn checked() -> Style {
        Style::default().fg(p::GREEN).add_modifier(Modifier::BOLD)
    }

    pub fn type_hint() -> Style {
        // Use TEXT_DIM color rather than Modifier::DIM — DIM modifier bleeds through popup renders
        // via ratatui's style-OR semantics (pre-Clear'd cells still OR the modifier into content
        // cells). Muting via color is visually equivalent and modifier-free.
        Style::default().fg(p::TEXT_DIM)
    }

    /// The popup's title text (`" columns "`). Same magenta accent as the border so the title
    /// reads as part of the popup chrome.
    pub fn title() -> Style {
        Style::default().fg(p::MAGENTA).add_modifier(Modifier::BOLD)
    }
}

/// Facet popup colors and styles.
pub mod facets {
    use super::base as p;
    use ratatui::style::{Modifier, Style};

    pub fn border() -> Style {
        Style::default().fg(p::BORDER_FOCUSED)
    }

    pub fn label() -> Style {
        Style::default()
            .fg(p::TEXT_MUTED)
            .add_modifier(Modifier::DIM)
    }

    pub fn value() -> Style {
        Style::default().fg(p::CYAN).add_modifier(Modifier::BOLD)
    }

    pub fn bar() -> Style {
        Style::default().fg(p::CYAN)
    }

    pub fn hint() -> Style {
        Style::default()
            .fg(p::TEXT_MUTED)
            .add_modifier(Modifier::DIM)
    }
}

/// Query-history popup colors and styles.
pub mod history {
    use super::base as p;
    use ratatui::style::{Modifier, Style};

    pub fn border() -> Style {
        Style::default().fg(p::BORDER_FOCUSED)
    }

    pub fn item() -> Style {
        Style::default().fg(p::TEXT)
    }

    pub fn selected() -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }

    pub fn hint() -> Style {
        Style::default()
            .fg(p::TEXT_MUTED)
            .add_modifier(Modifier::DIM)
    }
}

/// AI NL->SQL popup colors and styles.
pub mod ai {
    use super::base as p;
    use ratatui::style::{Modifier, Style};

    /// AI popup border — purple to mark it as the AI surface (still bright, distinct from the
    /// other focused popups).
    pub fn border() -> Style {
        Style::default().fg(p::PURPLE)
    }

    pub fn input() -> Style {
        Style::default().fg(p::TEXT)
    }

    pub fn pending() -> Style {
        Style::default().fg(p::YELLOW).add_modifier(Modifier::DIM)
    }

    pub fn success() -> Style {
        Style::default().fg(p::GREEN).add_modifier(Modifier::BOLD)
    }

    pub fn error() -> Style {
        Style::default().fg(p::ERROR).add_modifier(Modifier::BOLD)
    }

    pub fn hint() -> Style {
        Style::default()
            .fg(p::TEXT_MUTED)
            .add_modifier(Modifier::DIM)
    }
}

/// Grid (results table) colors and styles.
pub mod grid {
    use super::base as p;
    use ratatui::style::{Modifier, Style};

    /// Sticky header row — bright cyan + bold.
    pub fn header() -> Style {
        Style::default().fg(p::CYAN).add_modifier(Modifier::BOLD)
    }

    pub fn cell() -> Style {
        Style::default().fg(p::TEXT)
    }

    /// A SQL `NULL` cell's glyph — muted + dim so a null reads as "absent value".
    pub fn null() -> Style {
        Style::default()
            .fg(p::TEXT_MUTED)
            .add_modifier(Modifier::DIM)
    }

    pub fn stale_modifier() -> Modifier {
        Modifier::DIM
    }
}

/// Results pane chrome (border, row counter).
pub mod results {
    use super::base as p;
    use ratatui::style::{Color, Modifier, Style};

    /// Default row-counter style — bright cyan. Kept for callers that don't yet route through the
    /// state-aware [`row_counter_in`].
    pub fn row_counter() -> Style {
        row_counter_in(p::CYAN)
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
            .fg(p::TEXT_MUTED)
            .add_modifier(Modifier::DIM)
    }
}

#[cfg(test)]
mod theme_tests {
    use super::base as p;
    use super::border::{ResultState, mode_color, query_box, result_color, results};
    use crate::app::editor::EditorMode;
    use ratatui::style::Modifier;

    #[test]
    fn mode_color_maps_each_vim_mode_to_its_galaxy_accent() {
        assert_eq!(mode_color(EditorMode::Insert), p::CYAN);
        assert_eq!(mode_color(EditorMode::Normal), p::YELLOW);
        assert_eq!(mode_color(EditorMode::Operator('d')), p::GREEN);
        assert_eq!(
            mode_color(EditorMode::TextObject(
                'd',
                crate::app::editor::mode::TextObjectScope::Inner
            )),
            p::GREEN
        );
        assert_eq!(
            mode_color(EditorMode::CharSearch(
                crate::app::editor::char_search::SearchDirection::Forward,
                crate::app::editor::char_search::SearchType::Find
            )),
            p::PINK
        );
        assert_eq!(
            mode_color(EditorMode::OperatorCharSearch(
                'd',
                0,
                crate::app::editor::char_search::SearchDirection::Forward,
                crate::app::editor::char_search::SearchType::Find
            )),
            p::PINK
        );
    }

    #[test]
    fn query_box_focused_takes_mode_color_and_error_overrides() {
        // Focused, no error → mode color.
        assert_eq!(query_box(EditorMode::Insert, false, true).fg, Some(p::CYAN));
        assert_eq!(
            query_box(EditorMode::Normal, false, true).fg,
            Some(p::YELLOW)
        );
        // Focused, error → red overrides mode.
        assert_eq!(
            query_box(EditorMode::Normal, true, true).fg,
            Some(p::BORDER_ERROR)
        );
        assert_eq!(
            query_box(EditorMode::Insert, true, true).fg,
            Some(p::BORDER_ERROR)
        );
    }

    #[test]
    fn query_box_unfocused_recedes_in_muted_slate_regardless_of_mode_or_error() {
        for mode in [
            EditorMode::Insert,
            EditorMode::Normal,
            EditorMode::Operator('d'),
        ] {
            for has_error in [false, true] {
                assert_eq!(
                    query_box(mode, has_error, false).fg,
                    Some(p::BORDER_UNFOCUSED),
                    "unfocused dominates: mode={mode:?} has_error={has_error}"
                );
            }
        }
    }

    #[test]
    fn result_color_maps_each_state_to_its_galaxy_accent() {
        assert_eq!(result_color(ResultState::Ok), p::GREEN);
        assert_eq!(result_color(ResultState::Empty), p::YELLOW);
        assert_eq!(result_color(ResultState::Error), p::RED);
        assert_eq!(result_color(ResultState::Pending), p::CYAN);
    }

    #[test]
    fn results_focused_uses_state_color_unfocused_dims_same_hue() {
        // Focused: bright state color, no DIM.
        let focused_ok = results(ResultState::Ok, true);
        assert_eq!(focused_ok.fg, Some(p::GREEN));
        assert!(!focused_ok.add_modifier.contains(Modifier::DIM));
        // Unfocused: same hue, DIM applied.
        let unfocused_ok = results(ResultState::Ok, false);
        assert_eq!(unfocused_ok.fg, Some(p::GREEN));
        assert!(unfocused_ok.add_modifier.contains(Modifier::DIM));
        // Same pattern for the other states.
        for state in [ResultState::Empty, ResultState::Error, ResultState::Pending] {
            let focused = results(state, true);
            let unfocused = results(state, false);
            assert_eq!(focused.fg, unfocused.fg, "hue preserved across focus");
            assert!(!focused.add_modifier.contains(Modifier::DIM));
            assert!(unfocused.add_modifier.contains(Modifier::DIM));
        }
    }
}
