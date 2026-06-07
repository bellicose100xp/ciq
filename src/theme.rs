//! Centralized colors and styles (`dev/PLAN.md` §6.8 + project `CLAUDE.md` theme rule).
//!
//! Every color/style ciq paints lives here, grouped by surface (`grid`, and later `schema`,
//! `palette`, `facet`). Render files use `theme::<surface>::<CONST>` and never import
//! `ratatui::style::Color` or hardcode `Color::*` directly. Keeping it all in one place is
//! what lets a light/dark polarity pass (a §4.7 human-validated concern) be a single-file
//! change rather than a hunt across render code.

/// Grid (results table) colors and styles.
pub mod grid {
    use ratatui::style::{Color, Modifier, Style};

    /// The sticky header row: bold, in the accent color, so column names read as distinct from
    /// the data body.
    pub fn header() -> Style {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }

    /// Normal data-cell text.
    pub fn cell() -> Style {
        Style::default()
    }

    /// A SQL `NULL` cell's glyph — dimmed so a null reads as "absent value", visually distinct
    /// from an empty-string cell (which renders as nothing). The dim modifier is what carries
    /// the distinction in the real terminal; the glyph text itself (`col_width::NULL_GLYPH`)
    /// carries it headlessly.
    pub fn null() -> Style {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    }
}
