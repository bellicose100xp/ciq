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

    /// The query-bar cursor cell in **Insert** mode. Reverse-video so it reads as a visible block
    /// cursor regardless of terminal theme — and, crucially, so it shows up as a styled cell in a
    /// headless `TestBackend` snapshot (a `frame.set_cursor` cursor would not). Mirrors jiq's
    /// Insert-mode cursor. Color polarity is the §4.7 human-validated concern.
    pub fn cursor() -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }

    /// The query-bar cursor cell in vim **Normal** mode (and the other command modes). A colored
    /// reverse-video block (vs Insert's plain reverse) so the mode is legible at the cursor itself
    /// — the vim convention where the block cursor signals command mode. Mirrors jiq's per-mode
    /// cursor styling.
    pub fn cursor_normal() -> Style {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::REVERSED)
    }

    /// The vim mode badge shown at the right of the status line (`INSERT` / `NORMAL` / `d(` …) —
    /// bold accent so the current mode reads at a glance, distinct from the quiet status text.
    pub fn mode_indicator() -> Style {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
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

    /// The large-result truncation banner ("showing first N rows…") pinned at the top of the
    /// results pane when the grid is ciq-capped (P5.3). Accented so the cap reads at a glance, but
    /// not error-styled (truncation is normal, not a failure).
    pub fn truncation_banner() -> Style {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    }

    /// The empty-state notice shown in the results pane when there is no grid — the "type a query"
    /// hint or the "no rows match" result. Quiet, like the normal status, so it reads as a prompt
    /// not an alert.
    pub fn empty_state() -> Style {
        Style::default().fg(Color::DarkGray)
    }

    /// The `delim , | header on` dialect indicator shown in the results-pane border title — dimmed
    /// context, distinct from the grid's bold header. Color polarity (legibility light vs dark) is
    /// the §4.7 human-validated concern.
    pub fn dialect_summary() -> Style {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    }
}

/// Bottom keyboard-shortcut help-bar colors and styles (`dev/PLAN.md` §4.1, ported from jiq's
/// `help/help_line_render.rs`).
///
/// The help bar is the very bottom row: a context-sensitive list of `key  desc` pairs joined by a
/// `\u{2022}` bullet, prefixed by the vim mode badge when the query bar is focused. The key reads
/// as the accented foreground, the description as quiet metadata, and the bullet separator dimmer
/// still — so the whole line scans as a legend without competing with the grid above. Color
/// polarity (legibility light vs dark) is the §4.7 human-validated concern — centralizing here
/// keeps it a single-file change.
pub mod help_line {
    use ratatui::style::{Color, Modifier, Style};

    /// A shortcut key (`Tab`, `Ctrl+K`, `Esc`) — accented + bold so the actionable chord stands out.
    pub fn key() -> Style {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }

    /// A shortcut description (`complete`, `columns`) — quiet so it reads as the chord's label.
    pub fn description() -> Style {
        Style::default().fg(Color::DarkGray)
    }

    /// The `\u{2022}` bullet between hints — dimmest, a structural separator only.
    pub fn separator() -> Style {
        Style::default()
            .fg(Color::DarkGray)
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

/// Column-palette popup colors and styles (`dev/PLAN.md` §6.2, `dev/DECISIONS.md` D3).
///
/// The palette overlays the query bar: a bordered, fuzzy-filterable list of every column, each row
/// a checkbox + the column name + a right-aligned type badge. It reuses the autocomplete popup
/// chrome (border + selected reverse-video + dimmed type hint), so the styles mirror
/// [`autocomplete`]; the checked-checkbox accent is the palette-specific addition. Color polarity
/// (legibility light vs dark) is the §4.7 human-validated concern — centralizing here keeps it a
/// single-file change.
pub mod palette {
    use ratatui::style::{Color, Modifier, Style};

    /// The popup border / frame.
    pub fn border() -> Style {
        Style::default().fg(Color::DarkGray)
    }

    /// A normal (unselected) column row's text.
    pub fn item() -> Style {
        Style::default()
    }

    /// The row under the cursor — reverse video so it stands out regardless of terminal theme.
    pub fn selected() -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }

    /// A checked column's checkbox glyph — accented + bold so the selection set reads at a glance.
    pub fn checked() -> Style {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    }

    /// The right-aligned type badge (`int`/`txt`/…) — dimmed as secondary metadata.
    pub fn type_hint() -> Style {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM)
    }

    /// The popup title / footer hint line (the chord legend) — dimmed context.
    pub fn hint() -> Style {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    }
}

/// Facet popup colors and styles (`dev/PLAN.md` §6.5).
///
/// The facet popup overlays the query bar: a bordered box titled with the focused column, a few
/// stat lines (min/max/distinct/nulls), and — for a low-cardinality text column — a small top-K
/// value histogram (`value  count |####`). The stat labels read as quiet metadata; the values and
/// the histogram bars are the accented foreground. Color polarity (legibility light vs dark) is the
/// §4.7 human-validated concern — centralizing here keeps it a single-file change.
pub mod facets {
    use ratatui::style::{Color, Modifier, Style};

    /// The popup border / frame.
    pub fn border() -> Style {
        Style::default().fg(Color::DarkGray)
    }

    /// A stat label (`min`, `max`, `distinct`, `nulls`) — dimmed as secondary metadata so the value
    /// beside it draws the eye.
    pub fn label() -> Style {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    }

    /// A stat value (the min/max text, the counts) — the accented foreground.
    pub fn value() -> Style {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }

    /// A histogram bar (`####`) — accented so the distribution reads at a glance.
    pub fn bar() -> Style {
        Style::default().fg(Color::Cyan)
    }

    /// The popup title / pending ("computing…") line — dimmed context.
    pub fn hint() -> Style {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    }
}

/// Query-history popup colors and styles (`dev/PLAN.md` §7.6).
///
/// The history popup overlays the query bar: a bordered, fuzzy-filterable list of prior SQL
/// queries (newest first), the cursor row reverse-video, with a search needle in the title. It
/// reuses the autocomplete/palette popup chrome, so the styles mirror [`palette`]. Color polarity
/// (legibility light vs dark) is the §4.7 human-validated concern — centralizing here keeps it a
/// single-file change.
pub mod history {
    use ratatui::style::{Color, Modifier, Style};

    /// The popup border / frame.
    pub fn border() -> Style {
        Style::default().fg(Color::DarkGray)
    }

    /// A normal (unselected) history row's text.
    pub fn item() -> Style {
        Style::default()
    }

    /// The row under the cursor — reverse video so it stands out regardless of terminal theme.
    pub fn selected() -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }

    /// The "(no matches)" line and the popup title / footer hint — dimmed context.
    pub fn hint() -> Style {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    }
}

/// AI NL->SQL popup colors and styles (`dev/PLAN.md` §7 P5.1).
///
/// The AI popup overlays the query bar: a bordered box where the user types a natural-language
/// request, with a status line below it that shows the lifecycle (editing prompt / generating… /
/// the generated SQL / an error). It reuses the autocomplete/palette/history popup chrome, so the
/// styles mirror those surfaces. Color polarity (legibility light vs dark) is the §4.7
/// human-validated concern — centralizing here keeps it a single-file change.
pub mod ai {
    use ratatui::style::{Color, Modifier, Style};

    /// The popup border / frame.
    pub fn border() -> Style {
        Style::default().fg(Color::Magenta)
    }

    /// The natural-language input text the user is typing.
    pub fn input() -> Style {
        Style::default()
    }

    /// The "generating…" pending line — dimmed context.
    pub fn pending() -> Style {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::DIM)
    }

    /// The generated-SQL success line — accented so the produced query stands out.
    pub fn success() -> Style {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    }

    /// An error line — stands out from the normal status.
    pub fn error() -> Style {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    }

    /// The popup title / hint line — dimmed context.
    pub fn hint() -> Style {
        Style::default()
            .fg(Color::DarkGray)
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

    /// Modifier applied to the grid's header + body cells when the displayed result is stale —
    /// kept on screen after a query-pipeline error so the user can still see what they had while
    /// the error rides the status line (jiq's error-keeps-last-result-dimmed behavior). Returned
    /// as a [`Modifier`] (not a [`Style`]) so callers can OR it into existing per-cell styles
    /// (header/cell/null) without flattening their colors.
    pub fn stale_modifier() -> Modifier {
        Modifier::DIM
    }
}
