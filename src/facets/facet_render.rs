//! Facet popup blit + the pure `format_facets` line builder (`dev/PLAN.md` §6.5).
//!
//! Two pieces, the same compute/paint split every ciq render surface uses:
//!  - [`format_facets`] — **pure** `(&FacetResult, width) -> Vec<Line>`: the number/date/string
//!    formatting and the histogram **bar-width math** (`bar_len = count * inner_width / max_count`).
//!    Snapshot-tested directly (it touches no `Frame`).
//!  - [`render_facet`] — the **thin blit**: a bordered popup titled with the focused column, with
//!    [`format_facets`]'s lines inside. The §4.7 residue is the real-terminal glyphs / bar color
//!    only; the layout is all in `format_facets`.
//!
//! All colors come from [`theme::facets`] — this file never names a `Color`.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::theme;

use super::facet_state::{FacetResult, FacetState};

/// The glyph the histogram bars are drawn with (ASCII only, per the theme conventions).
const BAR_GLYPH: char = '#';

/// The fraction of the inner popup width given to the histogram bar (the rest holds the
/// `value  count ` prefix). Keeps the bars from crowding the labels in a narrow popup.
const BAR_WIDTH_FRACTION: usize = 2; // half the inner width

/// Render the facet popup into `area`: a bordered box titled with the focused column, with the
/// formatted stat / histogram lines inside. No-op on a degenerate area. While the result is still
/// in-flight it shows a dimmed "computing…" line.
pub fn render_facet(state: &FacetState, f: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let title = format!(
        " facet: {} ({}) ",
        state.column(),
        state.column_type().badge()
    );
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::facets::border())
        .style(theme::popup::surface())
        .title(Span::styled(title, theme::facets::hint()));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = match state.result() {
        Some(result) => format_facets(result, inner.width as usize),
        None => vec![Line::from(Span::styled(
            "computing…".to_string(),
            theme::facets::hint(),
        ))],
    };
    f.render_widget(Paragraph::new(lines), inner);
}

/// Build the styled facet lines for an inner width — **pure** (no `Frame`).
///
/// A summary renders `min` / `max` / `distinct` / `nulls` stat lines; a histogram renders
/// `distinct` / `nulls` then one `value  count |####` bar per top-K value, the bar length scaled
/// to the largest count. Each line is `width`-bounded.
pub fn format_facets(result: &FacetResult, width: usize) -> Vec<Line<'static>> {
    match result {
        FacetResult::Summary {
            min,
            max,
            distinct,
            nulls,
        } => vec![
            stat_line("min", &opt_value(min), width),
            stat_line("max", &opt_value(max), width),
            stat_line("distinct", &distinct.to_string(), width),
            stat_line("nulls", &nulls.to_string(), width),
        ],
        FacetResult::Histogram {
            bars,
            distinct,
            nulls,
        } => {
            let mut lines = vec![
                stat_line("distinct", &distinct.to_string(), width),
                stat_line("nulls", &nulls.to_string(), width),
            ];
            let max_count = result_max(bars);
            let bar_budget = bar_budget(width);
            // Left column holds `value  count ` before the bar; size it to the rest of the width.
            let label_budget = width.saturating_sub(bar_budget);
            for bar in bars {
                lines.push(bar_line(
                    &bar.value,
                    bar.count,
                    max_count,
                    label_budget,
                    bar_budget,
                ));
            }
            lines
        }
    }
}

/// A `label: value` stat line, the label dimmed and the value accented, padded to `width`.
fn stat_line(label: &str, value: &str, width: usize) -> Line<'static> {
    let label_part = format!("{label}: ");
    let value_part = truncate(value, width.saturating_sub(label_part.chars().count()));
    let used = label_part.chars().count() + value_part.chars().count();
    let gap = width.saturating_sub(used);
    Line::from(vec![
        Span::styled(label_part, theme::facets::label()),
        Span::styled(value_part, theme::facets::value()),
        Span::styled(" ".repeat(gap), theme::facets::label()),
    ])
}

/// One histogram bar line: `value  count |####` — the value + count in the label budget, then the
/// proportional bar filling the bar budget. The label is dimmed-ish (value style), the bar accented.
fn bar_line(
    value: &str,
    count: u64,
    max_count: u64,
    label_budget: usize,
    bar_budget: usize,
) -> Line<'static> {
    let count_str = count.to_string();
    // `value  count ` then the bar. Reserve room for the count + two separating spaces.
    let value_budget = label_budget.saturating_sub(count_str.chars().count() + 2);
    let value_part = truncate(value, value_budget);
    let label = format!("{value_part}  {count_str} ");
    let label = pad_to(&label, label_budget);

    let bar_len = bar_length(count, max_count, bar_budget);
    let bar: String = std::iter::repeat_n(BAR_GLYPH, bar_len).collect();
    let bar = pad_to(&bar, bar_budget);

    Line::from(vec![
        Span::styled(label, theme::facets::value()),
        Span::styled(bar, theme::facets::bar()),
    ])
}

/// The histogram bar width for `count`: `count * bar_budget / max_count`, clamped to `bar_budget`.
/// **Pure** — the §6.5 `bar_len = (count * inner_width) / max_count` formula. A zero `max_count`
/// (no bars / a summary) yields a zero-length bar (no divide-by-zero).
pub fn bar_length(count: u64, max_count: u64, bar_budget: usize) -> usize {
    if max_count == 0 || bar_budget == 0 {
        return 0;
    }
    let len = (count as u128 * bar_budget as u128) / max_count as u128;
    (len as usize).min(bar_budget)
}

/// The width reserved for the histogram bar (a fraction of the inner width), at least 1 when there
/// is any width at all.
fn bar_budget(width: usize) -> usize {
    if width == 0 {
        return 0;
    }
    (width / BAR_WIDTH_FRACTION).max(1)
}

/// The largest bar count (the histogram's `max_count` scale), `0` for an empty list.
fn result_max(bars: &[super::facet_state::FacetBar]) -> u64 {
    bars.iter().map(|b| b.count).max().unwrap_or(0)
}

/// Render an optional summary value, showing the themed null/absent glyph when the column is
/// entirely NULL (so `min`/`max` came back NULL).
fn opt_value(v: &Option<String>) -> String {
    match v {
        Some(s) => s.clone(),
        None => "(null)".to_string(),
    }
}

/// Truncate `s` to at most `max` chars, appending `…` when cut (the grid / popup ellipsis rule).
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
fn pad_to(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len > width {
        s.chars().take(width).collect()
    } else {
        format!("{s}{}", " ".repeat(width - len))
    }
}

#[cfg(test)]
#[path = "facet_render_tests.rs"]
mod facet_render_tests;
