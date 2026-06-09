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
}

/// Border styles, focus-aware (`dev/PLAN.md` §3.1 felt-loop polish).
pub mod border {
    use super::base as p;
    use ratatui::style::Style;

    /// The border of the currently-focused pane — bright cyan.
    pub fn focused() -> Style {
        Style::default().fg(p::BORDER_FOCUSED)
    }

    /// The border of an unfocused pane — muted slate so it recedes.
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
    use ratatui::style::{Modifier, Style};

    pub fn key() -> Style {
        Style::default().fg(p::CYAN).add_modifier(Modifier::BOLD)
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
        Style::default().fg(p::YELLOW).add_modifier(Modifier::DIM)
    }
}

/// Column-palette popup colors and styles. Module name preserved for callers — base palette
/// constants live in [`base`] above to keep this name free for the popup.
pub mod palette {
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

    pub fn checked() -> Style {
        Style::default().fg(p::GREEN).add_modifier(Modifier::BOLD)
    }

    pub fn type_hint() -> Style {
        Style::default().fg(p::YELLOW).add_modifier(Modifier::DIM)
    }

    pub fn hint() -> Style {
        Style::default()
            .fg(p::TEXT_MUTED)
            .add_modifier(Modifier::DIM)
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
    use ratatui::style::{Modifier, Style};

    /// `<rendered>/<total>` row counter pinned to the results-pane top-right border. Bright cyan
    /// so it reads as a quiet accent against the focused-cyan border.
    pub fn row_counter() -> Style {
        Style::default().fg(p::CYAN)
    }

    /// Row counter while the displayed result is stale — muted + dim to ride the stale-grid
    /// polarity.
    pub fn row_counter_stale() -> Style {
        Style::default()
            .fg(p::TEXT_MUTED)
            .add_modifier(Modifier::DIM)
    }
}
