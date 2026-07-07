//! Popup blit — `render_popup(state, frame, area)` over [`AutocompleteState`] (`dev/PLAN.md`
//! §5.1/§5.6).
//!
//! The reused jiq `render_popup`, retargeted to ciq's suggestion shape. It paints a bordered list
//! of candidates above/below the query bar; each row is the candidate text on the left and a
//! **right-aligned type-hint label** on the right (`int`/`date`/… for typed columns, `kw`/`fn`/
//! `agg`/`op`/`val` for the rest). The selected row is reverse-video.
//!
//! It is a **thin blit**: every layout decision (which rows, the label text) is a pure function
//! tested directly ([`type_hint_label`]), and the paint itself is `TestBackend`-snapshot-tested
//! (NOT shell-exempt — `TestBackend` is an in-memory cell grid an agent asserts; only true-terminal
//! glyphs/placement/color-polarity are the §4.7 human surface, §5.6). All colors come from
//! [`theme::autocomplete`] — this file never names a `Color`.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::theme;

use super::autocomplete_state::{AutocompleteState, Suggestion, SuggestionType};

/// Max popup rows shown at once (the visible window; the list may be longer and scroll with the
/// selection in a later pass — v1 shows the top slice around the selection).
pub const MAX_VISIBLE_ROWS: u16 = 8;

/// Render the popup into `area`. No-op when the popup is closed or `area` is degenerate.
///
/// `area` is the region the popup should occupy (the App computes it from the query-bar position
/// and the available space — see `App` popup placement). The popup draws a border and, inside it,
/// one line per visible candidate, the selected one reverse-video. The bottom border carries
/// context-sensitive hints (centered, with `\u{2022}` separators); when `show_columns_hint` is true
/// the SELECT-pane-only `Ctrl+P columns` hint is added (the dedicated column-picker palette).
pub fn render_popup(
    state: &AutocompleteState,
    f: &mut Frame,
    area: Rect,
    show_columns_hint: bool,
    hovered: Option<usize>,
) {
    if !state.is_open() || area.width == 0 || area.height == 0 {
        return;
    }

    let usable = area.width.saturating_sub(2) as usize;
    let hint_line = Line::from(hint_spans(show_columns_hint, usable)).centered();

    // Clear + an opaque surface fill so the grid never leaks through the popup (ratatui ORs
    // spans into the cells behind them; without this the highlighted/dim grid shows through).
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::autocomplete::border())
        .style(theme::popup::surface())
        .title_bottom(hint_line);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = popup_lines(state, inner.width, inner.height, hovered);
    f.render_widget(Paragraph::new(lines), inner);
}

/// The styled hint spans for the popup's bottom border. Tab accept and ↑↓ select are universal
/// autocomplete idioms — we deliberately don't surface them. Only the contextual hints stay:
/// `Ctrl+P multi-select` when `show_columns_hint` is true (focus on the SELECT pane) — revealing
/// the dedicated column-picker palette — and `Esc close` (always). Drops trailing hints whole on
/// narrow widths.
pub(crate) fn hint_spans(show_columns_hint: bool, max_width: usize) -> Vec<Span<'static>> {
    let key_style = theme::help_line::key();
    let desc_style = theme::help_line::description();
    let sep_style = theme::help_line::separator();

    // Tab-accept, ↑↓-select, and Esc-close are universal autocomplete idioms — omitted. The only
    // hint worth surfacing is the CONTEXTUAL `Ctrl+P multi-select`, which reveals the dedicated
    // column-picker palette when focus is on the SELECT pane (the chord is anchored there). Off the
    // SELECT pane there's nothing non-obvious to show, so the bottom border stays clean.
    let mut hints: Vec<(&'static str, &'static str)> = Vec::new();
    if show_columns_hint {
        hints.push(("Ctrl+P", "multi-select"));
    }

    let mut out: Vec<Span<'static>> = Vec::with_capacity(hints.len() * 4);
    let mut width = 0usize;
    for (k, d) in &hints {
        let lead_first = out.is_empty();
        let sep = if lead_first { " " } else { " \u{2022} " };
        let chunk = sep.chars().count() + k.chars().count() + 1 + d.chars().count();
        if width + chunk > max_width {
            break;
        }
        out.push(Span::styled(sep.to_string(), sep_style));
        out.push(Span::styled(*k, key_style));
        out.push(Span::raw(" "));
        out.push(Span::styled(*d, desc_style));
        width += chunk;
    }
    out
}

/// Build the styled candidate lines for an inner width/height: a window of [`MAX_VISIBLE_ROWS`]
/// (capped by `height`) rows scrolled so the selected row is always visible, each laid out as
/// `<text>…<right-aligned hint>` and styled (selected reverse-video, hint dimmed).
fn popup_lines(
    state: &AutocompleteState,
    width: u16,
    height: u16,
    hovered: Option<usize>,
) -> Vec<Line<'static>> {
    let visible = (MAX_VISIBLE_ROWS.min(height)) as usize;
    let suggestions = state.suggestions();
    // Share the window math with the click handler (`scroll_window`) so a click on a scrolled list
    // maps to the same absolute index the renderer drew here.
    let (start, end) =
        crate::scroll_window::visible_window(state.selected(), suggestions.len(), visible);

    suggestions[start..end]
        .iter()
        .enumerate()
        .map(|(offset, s)| {
            let idx = start + offset;
            row_line(s, width, idx == state.selected(), hovered == Some(idx))
        })
        .collect()
}

/// One candidate row, padded to `width`. Every row reserves a **1-column left gutter** so all rows
/// align: on the selected row that gutter holds the bright accent bar (`▌`) and the content sits on
/// the elevated selection band; on other rows it's a blank space and the content uses the item /
/// type-hint styles (a hovered row folds the hover background into every span, since each span sets
/// its own opaque bg). Content (text + right-aligned label) is laid out in the remaining `width-1`.
fn row_line(s: &Suggestion, width: u16, selected: bool, hovered: bool) -> Line<'static> {
    let content_w = (width as usize).saturating_sub(1); // column 0 is the gutter/bar
    let label = type_hint_label(s);
    let text = truncate(&s.text, content_w.saturating_sub(label.len() + 1).max(1));
    let used = text.chars().count() + label.chars().count();
    let gap = content_w.saturating_sub(used);

    if selected {
        let content = pad_or_truncate(&format!("{text}{}{label}", " ".repeat(gap)), content_w);
        Line::from(vec![
            Span::styled(
                theme::popup::BAR,
                theme::popup::selected_bar(theme::autocomplete::ACCENT),
            ),
            Span::styled(content, theme::autocomplete::selected()),
        ])
    } else {
        let row_bg = hovered.then(theme::popup::hover_bg);
        let patch = |style: ratatui::style::Style| match row_bg {
            Some(bg) => style.bg(bg),
            None => style,
        };
        Line::from(vec![
            Span::styled(" ", patch(theme::autocomplete::item())), // gutter aligns with the bar
            Span::styled(text, patch(theme::autocomplete::item())),
            Span::styled(" ".repeat(gap), patch(theme::autocomplete::item())),
            Span::styled(label, patch(theme::autocomplete::type_hint())),
        ])
    }
}

/// The right-aligned type-hint label for a suggestion: the [`ColumnType`](crate::schema::ColumnType)
/// badge (`int`/`num`/`date`/…) for a typed `Field`/`Value`, and a fixed short tag for the other
/// kinds (`kw`/`fn`/`agg`/`op`/`val`/`fld`). Pure — unit-tested directly.
pub fn type_hint_label(s: &Suggestion) -> String {
    match (s.suggestion_type, s.field_type.as_ref()) {
        (SuggestionType::Field, Some(ty)) | (SuggestionType::Value, Some(ty)) => {
            ty.badge().to_string()
        }
        (SuggestionType::Field, None) => "fld".to_string(),
        (SuggestionType::Value, None) => "val".to_string(),
        (SuggestionType::Function, _) => "fn".to_string(),
        (SuggestionType::Aggregate, _) => "agg".to_string(),
        (SuggestionType::Operator, _) => "op".to_string(),
        (SuggestionType::Keyword, _) => "kw".to_string(),
    }
}

/// Truncate `s` to at most `max` characters, appending `…` when cut (matching the grid's
/// ellipsis rule). Returns `s` unchanged when it already fits.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let keep = max.saturating_sub(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('…');
    out
}

/// Pad `s` with trailing spaces to exactly `width` chars, or truncate it to `width`.
fn pad_or_truncate(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len > width {
        s.chars().take(width).collect()
    } else {
        format!("{s}{}", " ".repeat(width - len))
    }
}

#[cfg(test)]
#[path = "autocomplete_render_tests.rs"]
mod autocomplete_render_tests;
