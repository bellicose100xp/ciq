//! AI chat popup blit — `render_ai(state, label, frame, area)` over [`AiState`] (`dev/PLAN.md` §7
//! P5.1).
//!
//! The popup is a conversation laid out jiq-style: a scrollable transcript of `you:` / `ai:`
//! turns with the prose **wrapped to the box width**, each answer's suggestions listed under it as
//! numbered blocks (`[n] kind` prefix, the SQL wrapped with a hanging indent, the model's note on
//! its own dimmed line), a horizontal rule between exchanges, and an input line pinned to the bottom.
//! The top border names the provider and model; the bottom border carries the shortcut legend,
//! which changes with keyboard focus.
//!
//! ## The row map is the renderer's
//!
//! Wrapping means one turn or suggestion can take several rows, and how many depends on the width,
//! which only the layout knows. So the renderer owns the row model: [`layout_rows`] flattens the
//! transcript into rows tagged with the suggestion they belong to, and everything positional —
//! the visible window, the click target, the cursor's row for auto-scroll, the total for scroll
//! clamping — is derived from it by pure functions the App calls with the last painted size. The
//! state keeps only the scroll offset and the cursor as a flattened suggestion index.
//!
//! A **thin blit**: every layout decision is a pure helper tested directly, and the paint itself is
//! `TestBackend`-snapshot-tested (NOT shell-exempt — `TestBackend` is an in-memory cell grid an agent
//! asserts; only true-terminal glyphs / placement / color polarity / the real chord are the §4.7
//! human surface). All colors come from [`theme::ai`] — this file never names a `Color`.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::theme;

use super::ai_state::{AiPhase, AiState};
use super::chat::{Role, Turn};
use super::suggestion::Suggestion;

/// The fewest inner rows the popup accepts: enough for a few turns plus the input line. The layout
/// grants a share of the results pane and never less than this (clamped to the pane on a short
/// terminal), so a taller terminal shows more of the conversation.
pub const POPUP_ROWS: u16 = 10;

/// Width the `you:` / `ai:` labels are padded to, so the prose in every turn starts in the same
/// column and a long conversation reads as a column of text rather than a ragged edge.
const LABEL_WIDTH: usize = 4;

/// The hanging indent under a suggestion's `[n] kind` prefix: `[1] optimize ` is 13 cells.
const SUGGESTION_INDENT: usize = 13;

/// One painted row of the transcript, tagged with the flattened suggestion it belongs to (its SQL
/// rows and its note row alike), so a click anywhere in the block picks it and the cursor band
/// covers the whole block.
#[derive(Debug, Clone)]
pub struct Row {
    pub line: Line<'static>,
    pub suggestion: Option<usize>,
}

/// Render the chat popup into `area`. `label` names the provider and model for the title. No-op on
/// a degenerate area (the caller checks [`AiState::is_open`]).
pub fn render_ai(state: &AiState, label: &str, f: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    f.render_widget(Clear, area);
    let border = if state.is_focused() {
        theme::ai::border()
    } else {
        theme::ai::border_unfocused()
    };
    let usable = area.width.saturating_sub(2) as usize;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border)
        .style(theme::popup::surface())
        .title(Span::styled(title(label), theme::ai::title()))
        .title_bottom(Line::from(hint_spans(hints(state), usable)).centered());
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // The input line always owns the bottom row; the transcript gets whatever is left.
    let transcript_rows = inner.height.saturating_sub(1);
    let mut lines = transcript_lines(state, inner.width, transcript_rows);
    // Pad so the input stays pinned to the bottom even with a short transcript.
    while lines.len() < transcript_rows as usize {
        lines.push(Line::from(""));
    }
    lines.push(input_line(state, inner.width));
    f.render_widget(Paragraph::new(lines), inner);
}

/// The top-border title: `AI · <provider · model>` (jiq puts the provider on the left of its title
/// and the model on the right; ciq's single label carries both).
pub fn title(label: &str) -> String {
    if label.trim().is_empty() {
        " AI ".to_string()
    } else {
        format!(" AI \u{b7} {} ", label.trim())
    }
}

/// The bottom-border shortcut legend for the current focus. Focused: everything the chat answers
/// to. Unfocused: how to get the keyboard here, the one shortcut that works from the bar, and how
/// to hide the box.
pub fn hints(state: &AiState) -> &'static [(&'static str, &'static str)] {
    if state.is_focused() {
        &[
            ("Enter", "send"),
            ("Alt+1-5", "apply"),
            ("Alt+\u{2191}\u{2193}", "select"),
            ("Ctrl+L", "new"),
            ("Esc", "back"),
            ("Ctrl+A", "close"),
        ]
    } else {
        &[("Ctrl+G", "ask"), ("Alt+1-5", "apply"), ("Ctrl+A", "close")]
    }
}

/// Styled `key desc` hint spans joined by ` • `, dropping trailing hints whole when they would not
/// fit in `max_width` (the same narrow-width policy the other popups' borders use).
pub fn hint_spans(hints: &[(&str, &str)], max_width: usize) -> Vec<Span<'static>> {
    let key_style = theme::help_line::key_in(theme::ai::accent());
    let desc_style = theme::help_line::description();
    let sep_style = theme::help_line::separator();
    let mut out: Vec<Span<'static>> = Vec::with_capacity(hints.len() * 4);
    let mut width = 0usize;
    for (k, d) in hints {
        let sep = if out.is_empty() { " " } else { " \u{2022} " };
        let chunk = sep.chars().count() + k.chars().count() + 1 + d.chars().count();
        if width + chunk > max_width {
            break;
        }
        out.push(Span::styled(sep.to_string(), sep_style));
        out.push(Span::styled((*k).to_string(), key_style));
        out.push(Span::raw(" "));
        out.push(Span::styled((*d).to_string(), desc_style));
        width += chunk;
    }
    out
}

/// The minimum inner rows the popup accepts (the layout grows it from here with the pane).
pub fn popup_rows_for(_state: &AiState) -> u16 {
    POPUP_ROWS
}

// --- the row model ---

/// Flatten the transcript into painted rows at `width`: for each turn its wrapped prose, then one
/// block per suggestion (prefix + wrapped SQL + dimmed note), a horizontal rule after each answer,
/// and finally the status row (error banner or the fresh-chat hint) when there is one. The
/// keyboard cursor is painted onto its block here, so the caller never re-derives it.
pub fn layout_rows(state: &AiState, width: u16) -> Vec<Row> {
    let width = width as usize;
    let cursor = state.selected();
    let mut rows = Vec::new();
    let mut flat = 0usize;
    let turns = state.chat().turns();
    for (ti, turn) in turns.iter().enumerate() {
        rows.extend(turn_rows(turn, width));
        for (i, sug) in turn.suggestions.iter().enumerate() {
            let is_cursor = cursor == Some(flat);
            rows.extend(
                suggestion_rows(i, sug, width, is_cursor)
                    .into_iter()
                    .map(|line| Row {
                        line,
                        suggestion: Some(flat),
                    }),
            );
            flat += 1;
        }
        // A rule after every answer but the last, so a long conversation reads as separate
        // exchanges rather than one run of text.
        if turn.role == Role::Assistant && ti + 1 < turns.len() {
            rows.push(Row {
                line: divider_line(width),
                suggestion: None,
            });
        }
    }
    if let Some(status) = status_line(state) {
        rows.push(Row {
            line: status,
            suggestion: None,
        });
    }
    rows
}

/// How many rows the whole transcript occupies at `width` — what scrolling clamps against.
pub fn row_count(state: &AiState, width: u16) -> usize {
    layout_rows(state, width).len()
}

/// The first row of the cursor's suggestion block at `width`, for keeping the cursor on screen.
pub fn selected_row(state: &AiState, width: u16) -> Option<usize> {
    let cursor = state.selected()?;
    layout_rows(state, width)
        .iter()
        .position(|r| r.suggestion == Some(cursor))
}

/// The visible transcript rows: [`layout_rows`] windowed to `height` rows from the state's scroll
/// offset, clamped so a short conversation shows from the top and an offset parked past the end
/// (see [`AiState::scroll_to_end`]) shows the last window.
pub fn transcript_lines(state: &AiState, width: u16, height: u16) -> Vec<Line<'static>> {
    if height == 0 {
        return Vec::new();
    }
    let rows = layout_rows(state, width);
    let start = window_start(state, rows.len(), height as usize);
    rows.into_iter()
        .skip(start)
        .take(height as usize)
        .map(|r| r.line)
        .collect()
}

/// The flattened suggestion a click on visible inner row `row` lands on, or `None` for prose, a
/// rule, the status row, the input line, or past the end. Uses the same window as the paint.
pub fn suggestion_at_row(state: &AiState, row: usize, width: u16, height: u16) -> Option<usize> {
    if row >= height as usize {
        return None;
    }
    let rows = layout_rows(state, width);
    let start = window_start(state, rows.len(), height as usize);
    rows.get(start + row)?.suggestion
}

/// The first visible row for the current offset: the offset clamped into `[0, total - height]`.
fn window_start(state: &AiState, total: usize, height: usize) -> usize {
    state.scroll().min(total.saturating_sub(height))
}

// --- rows ---

/// The horizontal rule between two exchanges, spanning the transcript width in the muted border
/// hue so it separates without competing with the text.
fn divider_line(width: usize) -> Line<'static> {
    Line::from(Span::styled("\u{2500}".repeat(width), theme::ai::divider()))
}

/// A turn's prose: the role label on the first row, then the text wrapped to the remaining width
/// with continuation rows indented under it.
fn turn_rows(turn: &Turn, width: usize) -> Vec<Row> {
    let label = format!(
        "{:<width$} ",
        format!("{}:", turn.role.label()),
        width = LABEL_WIDTH
    );
    let label_style = match turn.role {
        Role::User => theme::ai::user_label(),
        Role::Assistant => theme::ai::assistant_label(),
    };
    let indent = label.chars().count();
    let body_width = width.saturating_sub(indent).max(1);
    wrap_text(&turn.text, body_width)
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let lead = if i == 0 {
                Span::styled(label.clone(), label_style)
            } else {
                Span::styled(" ".repeat(indent), theme::ai::item())
            };
            Row {
                line: Line::from(vec![lead, Span::styled(text, theme::ai::input())]),
                suggestion: None,
            }
        })
        .collect()
}

/// A suggestion block: `[n] kind` then the SQL wrapped under a hanging indent, then the note on its
/// own dimmed row. The cursor block takes the shared popup selection band, with the accent bar on
/// its first row; other rows keep a blank gutter so text aligns.
pub fn suggestion_rows(
    idx: usize,
    s: &Suggestion,
    width: usize,
    is_cursor: bool,
) -> Vec<Line<'static>> {
    let kind_color = theme::ai::kind_color(s.kind);
    let prefix = format!("[{}] {:<8} ", idx + 1, s.kind.label());
    let indent = prefix.chars().count().max(SUGGESTION_INDENT);
    // One cell of gutter (the accent bar) on every row.
    let body_width = width.saturating_sub(1 + indent).max(1);
    let (text_style, note_style, gutter_style) = if is_cursor {
        (
            theme::ai::selected(),
            theme::ai::selected(),
            theme::ai::selected(),
        )
    } else {
        (
            theme::ai::kind_style(s.kind),
            theme::ai::note(),
            theme::ai::item(),
        )
    };
    let mut lines = Vec::new();
    for (i, text) in wrap_text(&s.sql, body_width).into_iter().enumerate() {
        let gutter = if i == 0 && is_cursor {
            Span::styled(theme::popup::BAR, theme::popup::selected_bar(kind_color))
        } else {
            Span::styled(" ", gutter_style)
        };
        let lead = if i == 0 {
            Span::styled(
                format!("{prefix:<indent$}"),
                if is_cursor {
                    theme::ai::selected()
                } else {
                    theme::ai::kind_style(s.kind)
                },
            )
        } else {
            Span::styled(" ".repeat(indent), gutter_style)
        };
        let padded = format!("{text:<body_width$}");
        lines.push(Line::from(vec![
            gutter,
            lead,
            Span::styled(padded, text_style),
        ]));
    }
    if !s.details.trim().is_empty() {
        for text in wrap_text(&s.details, body_width) {
            let padded = format!("{text:<body_width$}");
            lines.push(Line::from(vec![
                Span::styled(" ", gutter_style),
                Span::styled(" ".repeat(indent), gutter_style),
                Span::styled(padded, note_style),
            ]));
        }
    }
    lines
}

/// The bottom input row: a `> ` prompt then the line being typed, with a reverse-video cursor cell
/// after it while the chat has the keyboard (the same block cursor the query bar paints). A line
/// longer than the row scrolls so its tail and the cursor stay visible. While a request is in flight
/// the prompt is replaced by a muted waiting line, so the user can see the send landed. Visible but
/// unfocused with nothing typed, it says how to get the keyboard here.
pub fn input_line(state: &AiState, width: u16) -> Line<'static> {
    let width = width as usize;
    if state.is_pending() {
        return Line::from(Span::styled(
            pad_or_truncate("thinking…", width),
            theme::ai::pending(),
        ));
    }
    let body_width = width.saturating_sub(2);
    if state.is_open() && !state.is_focused() && state.input().is_empty() {
        return Line::from(vec![
            Span::styled("> ", theme::ai::hint()),
            Span::styled(
                pad_or_truncate("click here or press Ctrl+G to ask", body_width),
                theme::ai::pending(),
            ),
        ]);
    }
    if !state.is_focused() {
        return Line::from(vec![
            Span::styled("> ", theme::ai::hint()),
            Span::styled(
                pad_or_truncate(state.input(), body_width),
                theme::ai::input(),
            ),
        ]);
    }
    // Focused: the cursor cell sits right after the text and the rest of the row is padding.
    let text_width = body_width.saturating_sub(1);
    let shown = tail(state.input(), text_width);
    let shown_len = shown.chars().count();
    let mut spans = vec![
        Span::styled("> ", theme::ai::hint()),
        Span::styled(shown, theme::ai::input()),
    ];
    if body_width > 0 {
        spans.push(Span::styled(" ", theme::app::cursor()));
        spans.push(Span::styled(
            " ".repeat(text_width - shown_len),
            theme::ai::input(),
        ));
    }
    Line::from(spans)
}

/// The last `width` chars of `s` (all of it when it fits), so a long input line scrolls left.
fn tail(s: &str, width: usize) -> String {
    let len = s.chars().count();
    s.chars().skip(len.saturating_sub(width)).collect()
}

/// The row appended after the transcript: an error banner, or the empty-state hint before anything
/// has been said. `None` once the conversation is under way and nothing is wrong.
pub fn status_line(state: &AiState) -> Option<Line<'static>> {
    match state.phase() {
        AiPhase::Error(msg) => Some(Line::from(Span::styled(
            format!("error: {msg}"),
            theme::ai::error(),
        ))),
        _ if state.is_fresh() => Some(Line::from(Span::styled(
            "ask about your data, or describe the query you want".to_string(),
            theme::ai::pending(),
        ))),
        _ => None,
    }
}

// --- text helpers ---

/// Word-wrap `text` to `max_width` cells. Newlines in the source start new rows; a word longer than
/// the width is split hard so nothing is lost. Always returns at least one row (empty text gives one
/// empty row), so a turn never vanishes from the transcript.
pub fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let max_width = max_width.max(1);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        let mut current_len = 0usize;
        for word in paragraph.split_whitespace() {
            let word_len = word.chars().count();
            if current_len == 0 {
                push_word(&mut lines, &mut current, &mut current_len, word, max_width);
            } else if current_len + 1 + word_len <= max_width {
                current.push(' ');
                current.push_str(word);
                current_len += 1 + word_len;
            } else {
                lines.push(std::mem::take(&mut current));
                current_len = 0;
                push_word(&mut lines, &mut current, &mut current_len, word, max_width);
            }
        }
        lines.push(current);
    }
    // A source newline at the very end would leave a trailing empty row; drop it.
    while lines.len() > 1 && lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

/// Start `current` with `word`, hard-splitting any chunk wider than `max_width`.
fn push_word(
    lines: &mut Vec<String>,
    current: &mut String,
    current_len: &mut usize,
    word: &str,
    max_width: usize,
) {
    let chars: Vec<char> = word.chars().collect();
    let mut start = 0;
    while chars.len() - start > max_width {
        lines.push(chars[start..start + max_width].iter().collect());
        start += max_width;
    }
    *current = chars[start..].iter().collect();
    *current_len = chars.len() - start;
}

/// Pad `s` with trailing spaces to exactly `width` chars, or truncate it to `width` with a trailing
/// ellipsis (the grid/popup ellipsis rule).
fn pad_or_truncate(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len > width {
        if width == 0 {
            return String::new();
        }
        let keep = width.saturating_sub(1);
        let mut out: String = s.chars().take(keep).collect();
        out.push('…');
        return out;
    }
    let mut out = s.to_string();
    out.extend(std::iter::repeat_n(' ', width - len));
    out
}

#[cfg(test)]
#[path = "ai_render_tests.rs"]
mod ai_render_tests;
