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
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::theme;

use super::autocomplete_state::{AutocompleteState, Suggestion, SuggestionType};

/// Max popup rows shown at once (the visible window; the list may be longer and scroll with the
/// selection in a later pass — v1 shows the top slice around the selection).
pub const MAX_VISIBLE_ROWS: u16 = 8;

/// Render the popup into `area`. No-op when the popup is closed or `area` is degenerate.
///
/// `area` is the region the popup should occupy (the App computes it from the query-bar position
/// and the available space — see `App` popup placement). The popup draws a border and, inside it,
/// one line per visible candidate, the selected one reverse-video.
pub fn render_popup(state: &AutocompleteState, f: &mut Frame, area: Rect) {
    if !state.is_open() || area.width == 0 || area.height == 0 {
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::autocomplete::border());
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = popup_lines(state, inner.width, inner.height);
    f.render_widget(Paragraph::new(lines), inner);
}

/// Build the styled candidate lines for an inner width/height: a window of [`MAX_VISIBLE_ROWS`]
/// (capped by `height`) rows scrolled so the selected row is always visible, each laid out as
/// `<text>…<right-aligned hint>` and styled (selected reverse-video, hint dimmed).
fn popup_lines(state: &AutocompleteState, width: u16, height: u16) -> Vec<Line<'static>> {
    let visible = (MAX_VISIBLE_ROWS.min(height)) as usize;
    let suggestions = state.suggestions();
    let (start, end) = visible_window(state.selected(), suggestions.len(), visible);

    suggestions[start..end]
        .iter()
        .enumerate()
        .map(|(offset, s)| {
            let idx = start + offset;
            row_line(s, width, idx == state.selected())
        })
        .collect()
}

/// The `[start, end)` slice of candidate indices to show: a window of `visible` rows that keeps
/// `selected` in view (scrolls only when the selection would fall outside the top window).
fn visible_window(selected: usize, len: usize, visible: usize) -> (usize, usize) {
    if len <= visible {
        return (0, len);
    }
    // Keep the selection inside [start, start+visible). Anchor the window so `selected` is the
    // last row once it scrolls past the first window, but never past the end.
    let start = if selected < visible {
        0
    } else {
        (selected + 1).saturating_sub(visible).min(len - visible)
    };
    (start, start + visible)
}

/// One candidate row, padded to `width`: the candidate text left-aligned, the type-hint label
/// right-aligned, the gap filled with spaces. Selected rows are reverse-video (the whole line);
/// otherwise the text uses the item style and the hint the dimmed type-hint style.
fn row_line(s: &Suggestion, width: u16, selected: bool) -> Line<'static> {
    let width = width as usize;
    let label = type_hint_label(s);
    let text = truncate(&s.text, width.saturating_sub(label.len() + 1).max(1));
    let used = text.chars().count() + label.chars().count();
    let gap = width.saturating_sub(used);

    if selected {
        // The whole row in reverse video reads as one highlighted band.
        let content = format!("{text}{}{label}", " ".repeat(gap));
        let content = pad_or_truncate(&content, width);
        Line::from(Span::styled(content, theme::autocomplete::selected()))
    } else {
        Line::from(vec![
            Span::styled(text, theme::autocomplete::item()),
            Span::styled(" ".repeat(gap), theme::autocomplete::item()),
            Span::styled(label, theme::autocomplete::type_hint()),
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
