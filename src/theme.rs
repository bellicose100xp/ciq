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

/// Autocomplete popup colors and styles (`dev/PLAN.md` §5.1/§5.6).
///
/// The popup overlays the query bar: a bordered list of candidates, each a candidate text plus a
/// right-aligned type-hint label (`int`/`date`/`kw`/`fn`/`agg`/`op`/…). The selected row is
/// reverse-video so it stands out; the type-hint column is dimmed so it reads as secondary
/// metadata against the candidate text. Color polarity (legibility light vs dark) is the §4.7
/// human-validated concern — centralizing here keeps that a single-file change.
pub mod autocomplete {
    use ratatui::style::{Color, Modifier, Style};

    /// The popup border / frame.
    pub fn border() -> Style {
        Style::default().fg(Color::DarkGray)
    }

    /// A normal (unselected) candidate row's text.
    pub fn item() -> Style {
        Style::default()
    }

    /// The selected candidate row — reverse video so it stands out regardless of terminal theme.
    pub fn selected() -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }

    /// The right-aligned type-hint label (`int`/`kw`/`fn`/…) — dimmed as secondary metadata.
    pub fn type_hint() -> Style {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM)
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
