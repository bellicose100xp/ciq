//! Tests for `ai_render` — the pure row model (wrapping, blocks, spacers, status), the window and
//! click math derived from it, the title and bottom hints, the input line, and the popup blit
//! (`insta` + `ratatui::TestBackend`, logical cells only).
//!
//! The snapshots prove the *logical* cell grid. True-terminal glyphs, popup placement against a real
//! screen, the purple border color, and the real `Ctrl+A` chord are the §4.7 human surface, NOT
//! asserted here.

use super::*;
use crate::ai::ai_state::AiState;
use crate::ai::suggestion::{ChatReply, Suggestion, SuggestionKind};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

const LABEL: &str = "bedrock \u{b7} global.anthropic.claude-fable-5-1";

fn render(state: &AiState, w: u16, h: u16, area: Rect) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).expect("TestBackend");
    t.draw(|f| render_ai(state, LABEL, f, area)).expect("draw");
    t.backend().to_string()
}

fn sug(kind: SuggestionKind, sql: &str, details: &str) -> Suggestion {
    Suggestion {
        kind,
        sql: sql.to_string(),
        details: details.to_string(),
    }
}

fn text_of(line: &Line<'_>) -> String {
    line.spans.iter().map(|sp| sp.content.as_ref()).collect()
}

fn texts(rows: &[Row]) -> Vec<String> {
    rows.iter().map(|r| text_of(&r.line)).collect()
}

fn typed(s: &mut AiState, text: &str) {
    for c in text.chars() {
        s.push_char(c);
    }
}

/// An open chat holding one exchange with a fix and an optimize suggestion.
fn answered() -> AiState {
    let mut s = AiState::new();
    s.open();
    typed(&mut s, "only EU rows");
    s.send();
    s.set_reply(ChatReply {
        reply: "filter on the region column".to_string(),
        suggestions: vec![
            sug(
                SuggestionKind::Fix,
                "SELECT * FROM t WHERE region='EU'",
                "adds the filter",
            ),
            sug(
                SuggestionKind::Optimize,
                "SELECT id FROM t",
                "projects less",
            ),
        ],
    });
    s
}

/// Six exchanges, one suggestion each, for scrolling tests.
fn long() -> AiState {
    let mut s = AiState::new();
    s.open();
    for i in 0..6 {
        typed(&mut s, &format!("q{i}"));
        s.send();
        s.set_reply(ChatReply {
            reply: format!("a{i}"),
            suggestions: vec![sug(SuggestionKind::Fix, &format!("SELECT {i}"), "n")],
        });
    }
    s
}

// --- chrome ---

#[test]
fn title_names_the_provider_and_model() {
    assert_eq!(title(LABEL), format!(" AI \u{b7} {LABEL} "));
    assert_eq!(title(""), " AI ", "no label, no separator");
    assert_eq!(title("  x  "), " AI \u{b7} x ");
}

#[test]
fn hints_follow_focus() {
    let mut s = answered();
    let focused: Vec<&str> = hints(&s).iter().map(|(k, _)| *k).collect();
    assert_eq!(
        focused,
        vec![
            "Enter",
            "Alt+1-5",
            "Alt+\u{2191}\u{2193}",
            "Ctrl+L",
            "Esc",
            "Ctrl+A"
        ]
    );
    s.blur();
    let unfocused: Vec<&str> = hints(&s).iter().map(|(k, _)| *k).collect();
    assert_eq!(unfocused, vec!["Ctrl+G", "Alt+1-5", "Ctrl+A"]);
}

#[test]
fn hint_spans_drop_trailing_hints_that_do_not_fit() {
    let all = hint_spans(hints(&answered()), 200);
    let all_text: String = all.iter().map(|sp| sp.content.as_ref()).collect();
    assert!(all_text.contains("Enter send"), "{all_text}");
    assert!(all_text.contains("Ctrl+A close"), "{all_text}");
    let narrow = hint_spans(hints(&answered()), 24);
    let narrow_text: String = narrow.iter().map(|sp| sp.content.as_ref()).collect();
    assert!(narrow_text.contains("Enter send"), "{narrow_text}");
    assert!(
        !narrow_text.contains("Ctrl+A"),
        "later hints are dropped whole: {narrow_text}"
    );
    assert!(hint_spans(hints(&answered()), 0).is_empty());
}

// --- wrapping ---

#[test]
fn wrap_text_breaks_on_words_and_keeps_every_char() {
    assert_eq!(
        wrap_text("the quick brown fox jumps", 10),
        vec!["the quick", "brown fox", "jumps"]
    );
    assert_eq!(wrap_text("short", 40), vec!["short"]);
    assert_eq!(
        wrap_text("", 10),
        vec![""],
        "empty text still occupies a row"
    );
}

#[test]
fn wrap_text_splits_an_overlong_word_rather_than_dropping_it() {
    assert_eq!(wrap_text("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
    assert_eq!(
        wrap_text("x abcdefghij y", 4),
        vec!["x", "abcd", "efgh", "ij y"]
    );
}

#[test]
fn wrap_text_honors_source_newlines_and_multibyte_chars() {
    assert_eq!(wrap_text("a\nb c", 10), vec!["a", "b c"]);
    assert_eq!(wrap_text("日本語 テキスト", 4), vec!["日本語", "テキスト"]);
    assert_eq!(
        wrap_text("a\n", 10),
        vec!["a"],
        "a trailing newline adds no row"
    );
}

// --- the row model ---

#[test]
fn layout_rows_lists_prose_then_suggestion_blocks_then_the_status() {
    let s = answered();
    let rows = layout_rows(&s, 60);
    let t = texts(&rows);
    assert!(t[0].starts_with("you: only EU rows"), "{t:?}");
    assert!(
        t[1].starts_with("ai:  filter on the region column"),
        "{t:?}"
    );
    assert!(
        t[2].contains("[1] fix") && t[2].contains("SELECT * FROM t"),
        "{t:?}"
    );
    assert!(
        t[3].contains("adds the filter"),
        "the note gets its own row: {t:?}"
    );
    assert!(t[4].contains("[2] optimize"), "{t:?}");
    assert!(t[5].contains("projects less"), "{t:?}");
    assert_eq!(
        rows.len(),
        6,
        "no spacer after the last answer, no status row mid-chat"
    );
    // Every row of a block carries its suggestion; prose carries none.
    assert_eq!(rows[0].suggestion, None);
    assert_eq!(rows[2].suggestion, Some(0));
    assert_eq!(
        rows[3].suggestion,
        Some(0),
        "the note row belongs to the block too"
    );
    assert_eq!(rows[4].suggestion, Some(1));
}

#[test]
fn layout_rows_wraps_long_prose_and_sql_to_the_width() {
    let mut s = AiState::new();
    s.open();
    typed(&mut s, "q");
    s.send();
    s.set_reply(ChatReply {
        reply: "one two three four five six seven eight nine ten".to_string(),
        suggestions: vec![sug(
            SuggestionKind::Optimize,
            "SELECT region, status, count(*) AS n FROM t GROUP BY region, status ORDER BY n DESC",
            "",
        )],
    });
    let rows = layout_rows(&s, 30);
    let t = texts(&rows);
    let prose: Vec<&String> = t
        .iter()
        .filter(|l| !l.contains("SELECT") && !l.trim().is_empty())
        .collect();
    assert!(
        prose.len() >= 3,
        "the prose wrapped over several rows: {t:?}"
    );
    assert!(
        t.iter().all(|l| l.chars().count() <= 30),
        "no row exceeds the width: {t:?}"
    );
    let sql_rows: Vec<&Row> = rows.iter().filter(|r| r.suggestion == Some(0)).collect();
    assert!(sql_rows.len() >= 3, "the SQL wrapped: {t:?}");
    let sql_text: String = sql_rows
        .iter()
        .map(|r| text_of(&r.line))
        .collect::<Vec<_>>()
        .join(" ");
    let words: Vec<&str> = sql_text.split_whitespace().collect();
    for w in ["SELECT", "region,", "count(*)", "ORDER", "BY", "n", "DESC"] {
        assert!(
            words.contains(&w),
            "nothing was truncated away, missing {w}: {t:?}"
        );
    }
    // An empty note adds no row: every row of the block carries SQL text.
    assert!(sql_rows.iter().all(|r| !text_of(&r.line).trim().is_empty()));
}

#[test]
fn layout_rows_puts_a_spacer_between_exchanges() {
    let s = long();
    let rows = layout_rows(&s, 60);
    let t = texts(&rows);
    let blanks = t.iter().filter(|l| l.is_empty()).count();
    assert_eq!(blanks, 5, "a spacer after every answer but the last");
    assert!(!t.last().unwrap().is_empty(), "no trailing spacer");
}

#[test]
fn layout_rows_appends_the_hint_or_the_error() {
    let mut s = AiState::new();
    s.open();
    let t = texts(&layout_rows(&s, 60));
    assert_eq!(t.len(), 1);
    assert!(t[0].contains("ask about your data"));
    typed(&mut s, "q");
    s.send();
    s.set_error("HTTP 500");
    let t = texts(&layout_rows(&s, 60));
    assert!(t.last().unwrap().contains("HTTP 500"));
}

#[test]
fn row_count_and_selected_row_follow_the_layout() {
    let mut s = answered();
    assert_eq!(row_count(&s, 60), 6);
    assert_eq!(selected_row(&s, 60), None);
    s.select(1);
    assert_eq!(
        selected_row(&s, 60),
        Some(4),
        "the first row of the second block"
    );
    // A narrower width wraps the first block's SQL, pushing the second block down.
    assert!(selected_row(&s, 24).unwrap() > 4);
}

#[test]
fn the_cursor_block_takes_the_accent_bar_on_its_first_row_only() {
    let mut s = answered();
    s.select(0);
    let rows = layout_rows(&s, 60);
    assert_eq!(
        rows[2].line.spans.first().map(|sp| sp.content.as_ref()),
        Some(crate::theme::popup::BAR)
    );
    assert_eq!(
        rows[3].line.spans.first().map(|sp| sp.content.as_ref()),
        Some(" "),
        "the note row keeps a blank gutter"
    );
    assert_eq!(
        rows[4].line.spans.first().map(|sp| sp.content.as_ref()),
        Some(" ")
    );
}

#[test]
fn the_two_kinds_take_their_own_hues() {
    assert_ne!(
        crate::theme::ai::kind_color(SuggestionKind::Fix),
        crate::theme::ai::kind_color(SuggestionKind::Optimize)
    );
}

// --- window + click ---

#[test]
fn transcript_lines_window_to_the_height() {
    let s = answered();
    assert_eq!(transcript_lines(&s, 60, 2).len(), 2);
    assert_eq!(transcript_lines(&s, 60, 0).len(), 0);
    assert_eq!(transcript_lines(&s, 60, 50).len(), 6, "never invents rows");
}

#[test]
fn a_short_conversation_shows_from_the_top_whatever_the_offset() {
    let mut s = answered();
    s.scroll_to_end();
    let rows = transcript_lines(&s, 60, 10);
    assert!(
        text_of(&rows[0]).starts_with("you:"),
        "{}",
        text_of(&rows[0])
    );
}

#[test]
fn scrolling_a_long_conversation_moves_the_window() {
    let mut s = long();
    let total = row_count(&s, 60);
    let bottom = transcript_lines(&s, 60, 4);
    let bottom_text: Vec<String> = bottom.iter().map(text_of).collect();
    assert!(
        bottom_text.iter().any(|l| l.contains("SELECT 5")),
        "parked at the tail after an answer: {bottom_text:?}"
    );
    s.scroll_up(100, 4, total);
    let top = transcript_lines(&s, 60, 4);
    assert!(text_of(&top[0]).contains("q0"), "{}", text_of(&top[0]));
}

#[test]
fn suggestion_at_row_maps_every_row_of_a_block_and_nothing_else() {
    let s = answered();
    assert_eq!(suggestion_at_row(&s, 0, 60, 10), None, "prose");
    assert_eq!(suggestion_at_row(&s, 2, 60, 10), Some(0), "SQL row");
    assert_eq!(
        suggestion_at_row(&s, 3, 60, 10),
        Some(0),
        "note row of the same block"
    );
    assert_eq!(suggestion_at_row(&s, 4, 60, 10), Some(1));
    assert_eq!(suggestion_at_row(&s, 6, 60, 10), None, "past the end");
    assert_eq!(
        suggestion_at_row(&s, 2, 60, 2),
        None,
        "the input line is not a row"
    );
    assert_eq!(suggestion_at_row(&s, 2, 60, 0), None);
}

#[test]
fn suggestion_at_row_follows_the_scroll_offset() {
    let s = long();
    let clicked = (0..3).find_map(|r| suggestion_at_row(&s, r, 60, 3));
    assert_eq!(
        clicked,
        Some(5),
        "at the tail the visible block is the newest"
    );
}

// --- the input line ---

#[test]
fn the_input_line_shows_prompt_typed_text_pending_and_the_unfocused_hint() {
    let mut s = answered();
    typed(&mut s, "now group by status");
    let text = text_of(&input_line(&s, 40));
    assert!(text.starts_with("> now group by status"), "{text}");
    s.send();
    assert!(text_of(&input_line(&s, 40)).contains("thinking"));
    let mut s = answered();
    s.blur();
    let hint = text_of(&input_line(&s, 60));
    assert!(hint.contains("Ctrl+G"), "{hint}");
    s.focus();
    assert!(!text_of(&input_line(&s, 60)).contains("Ctrl+G"));
}

#[test]
fn the_focused_input_paints_a_block_cursor_after_the_text() {
    let mut s = answered();
    typed(&mut s, "abc");
    let line = input_line(&s, 20);
    assert_eq!(
        text_of(&line).chars().count(),
        20,
        "the row is filled to the width"
    );
    let cursor = line
        .spans
        .iter()
        .position(|sp| sp.style == crate::theme::app::cursor())
        .expect("a cursor cell");
    let before: String = line.spans[..cursor]
        .iter()
        .map(|sp| sp.content.as_ref())
        .collect();
    assert_eq!(
        before, "> abc",
        "the cursor sits right after the typed text"
    );
    s.blur();
    assert!(
        input_line(&s, 20)
            .spans
            .iter()
            .all(|sp| sp.style != crate::theme::app::cursor()),
        "no cursor without the keyboard"
    );
}

#[test]
fn a_long_focused_input_scrolls_so_the_cursor_stays_visible() {
    let mut s = answered();
    typed(&mut s, "0123456789abcdef");
    let text = text_of(&input_line(&s, 10));
    assert_eq!(text.chars().count(), 10);
    assert!(text.starts_with("> 9abcdef"), "the tail is shown: {text:?}");
    // Degenerate widths never panic or overflow.
    for w in [0u16, 1, 2, 3] {
        let _ = input_line(&s, w);
    }
}

#[test]
fn an_unfocused_chat_dims_its_border() {
    assert_ne!(
        crate::theme::ai::border().fg,
        crate::theme::ai::border_unfocused().fg
    );
    let mut s = answered();
    s.blur();
    let mut t = Terminal::new(TestBackend::new(80, 24)).expect("TestBackend");
    t.draw(|f| render_ai(&s, LABEL, f, Rect::new(0, 1, 62, 5)))
        .expect("draw");
    let corner = &t.backend().buffer()[(0, 1)];
    assert_eq!(corner.fg, crate::theme::ai::border_unfocused().fg.unwrap());
}

// --- the blit ---

#[test]
fn popup_rows_is_the_minimum_chat_height() {
    assert_eq!(popup_rows_for(&answered()), POPUP_ROWS);
}

#[test]
fn snapshot_ai_chat_80x24() {
    let s = answered();
    let screen = render(&s, 80, 24, Rect::new(10, 1, 70, 12));
    insta::assert_snapshot!(screen);
}

#[test]
fn snapshot_ai_chat_cursor_wrapped_80x24() {
    let mut s = answered();
    s.select(0);
    typed(&mut s, "now group by status");
    let screen = render(&s, 80, 24, Rect::new(30, 1, 50, 12));
    insta::assert_snapshot!(screen);
}

#[test]
fn snapshot_ai_chat_fresh_unfocused_80x24() {
    let mut s = AiState::new();
    s.open_unfocused();
    let screen = render(&s, 80, 24, Rect::new(10, 1, 70, 6));
    insta::assert_snapshot!(screen);
}

#[test]
fn snapshot_ai_chat_pending_80x24() {
    let mut s = AiState::new();
    s.open();
    typed(&mut s, "top 10 by amount");
    s.send();
    let screen = render(&s, 80, 24, Rect::new(10, 1, 70, 6));
    insta::assert_snapshot!(screen);
}

#[test]
fn render_no_op_on_zero_area() {
    let s = answered();
    let screen = render(&s, 10, 3, Rect::new(0, 0, 0, 0));
    assert!(
        screen.chars().all(|c| c == ' ' || c == '\n' || c == '"'),
        "zero area paints nothing, got:\n{screen}"
    );
}

#[test]
fn render_does_not_panic_on_degenerate_area() {
    let s = answered();
    for (w, h) in [(1u16, 1u16), (2, 2), (3, 1), (1, 3)] {
        let _ = render(&s, w.max(1), h.max(1), Rect::new(0, 0, w, h));
    }
}
