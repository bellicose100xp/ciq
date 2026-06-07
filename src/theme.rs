//! Centralized colors and styles (`dev/PLAN.md` §6.8 + project `CLAUDE.md` theme rule).
//!
//! Every color/style ciq paints lives here, grouped by surface (`grid`, and later `schema`,
//! `palette`, `facet`). Render files use `theme::<surface>::<CONST>` and never import
//! `ratatui::style::Color` or hardcode `Color::*` directly. Keeping it all in one place is
//! what lets a light/dark polarity pass (a §4.7 human-validated concern) be a single-file
//! change rather than a hunt across render code.

/// App shell colors and styles (query bar, status line, prompts).
pub mod app {
    use ratatui::style::{Color, Modifier, Style};

    /// The query bar's leading prompt glyph / label.
    pub fn prompt() -> Style {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    }

    /// The query text the user is typing.
    pub fn query_text() -> Style {
        Style::default()
    }

    /// A normal (informational) status line: "N rows", "ready", "running…".
    pub fn status() -> Style {
        Style::default().fg(Color::DarkGray)
    }

    /// An error status line (invalid SQL, load failure) — stands out from the normal status.
    pub fn status_error() -> Style {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    }

    /// The transient "loading CSV…" indicator shown in the results area during ingest.
    pub fn loading() -> Style {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::DIM)
    }
}

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
