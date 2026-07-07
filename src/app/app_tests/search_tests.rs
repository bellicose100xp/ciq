//! `App`-shell tests for the `Ctrl+F` row-filter search (routing, filtering, the display seam,
//! and the interaction with the facet `f` chord). Split out of `app_tests.rs` like the other
//! per-feature test files; the shared App helpers live in the parent (`super`).

use crate::app::{App, Focus, Key, KeyEvent, KeyMods};
use crate::engine::types::{Cell, Column, Table};
use crate::query::worker::types::{ProcessedResult, QueryRequest, QueryResponse, RequestKind};
use crate::schema::ColumnType;

use super::{loaded_app, type_str};

fn ctrl(key: Key) -> KeyEvent {
    KeyEvent::new(key, KeyMods::CTRL)
}

/// A 3-row result whose `region` column distinguishes rows: EU-WEST / NA / EU-EAST.
fn region_result() -> ProcessedResult {
    let table = Table::new(vec![
        Column::new(
            "id",
            ColumnType::Int,
            vec![Cell::Int(1), Cell::Int(2), Cell::Int(3)],
        ),
        Column::new(
            "region",
            ColumnType::Text,
            vec![
                Cell::Text("EU-WEST".into()),
                Cell::Text("NA".into()),
                Cell::Text("EU-EAST".into()),
            ],
        ),
    ]);
    let schema = table.schema();
    ProcessedResult::new(table, schema, 0)
}

fn app_with_regions() -> (App, std::sync::mpsc::Receiver<QueryRequest>) {
    let (mut app, rx) = loaded_app();
    type_str(&mut app, "SELECT * FROM t", 0);
    app.tick(150);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: region_result(),
        request_id: id,
        kind: RequestKind::Main,
    });
    if app.autocomplete().is_open() {
        app.on_key(KeyEvent::plain(Key::Esc), 200);
    }
    (app, rx)
}

fn type_needle(app: &mut App, s: &str) {
    for c in s.chars() {
        app.on_key(KeyEvent::char(c), 300);
    }
}

// --- open / close routing ---

#[test]
fn ctrl_f_opens_search_and_focuses_results() {
    let (mut app, _rx) = app_with_regions();
    assert!(!app.search().is_visible());
    app.on_key(ctrl(Key::Char('f')), 300);
    assert!(app.search().is_visible());
    assert!(app.search().is_editing());
    assert_eq!(app.focus(), Focus::Results);
}

#[test]
fn ctrl_f_without_result_is_a_noop() {
    let (mut app, _rx) = loaded_app();
    app.on_key(ctrl(Key::Char('f')), 0);
    assert!(!app.search().is_visible(), "nothing to filter yet");
}

#[test]
fn ctrl_f_no_longer_opens_the_facet_popup() {
    let (mut app, _rx) = app_with_regions();
    app.on_key(KeyEvent::plain(Key::Down), 300); // focus results
    assert_eq!(app.focus(), Focus::Results);
    app.on_key(ctrl(Key::Char('f')), 300);
    assert!(!app.is_facet_open(), "Ctrl+F is search, not the facet");
    assert!(app.search().is_visible());
}

#[test]
fn bare_f_in_results_still_opens_the_facet() {
    let (mut app, _rx) = app_with_regions();
    app.on_key(KeyEvent::plain(Key::Down), 300); // focus results
    app.on_key(KeyEvent::char('f'), 300);
    assert!(app.is_facet_open(), "modifier-free f keeps the facet chord");
    assert!(!app.search().is_visible());
}

#[test]
fn esc_while_editing_closes_and_clears() {
    let (mut app, _rx) = app_with_regions();
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    app.on_key(KeyEvent::plain(Key::Esc), 300);
    assert!(!app.search().is_visible());
    assert_eq!(app.search().needle(), "");
    assert_eq!(
        app.display_rows().unwrap().row_count(),
        3,
        "the unfiltered grid is restored"
    );
}

#[test]
fn ctrl_c_while_editing_still_quits() {
    let (mut app, _rx) = app_with_regions();
    app.on_key(ctrl(Key::Char('f')), 300);
    assert!(app.on_key(ctrl(Key::Char('c')), 300));
}

// --- live filtering ---

#[test]
fn typing_filters_rows_any_column_case_insensitive() {
    let (mut app, _rx) = app_with_regions();
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    let rows = app.display_rows().unwrap();
    assert_eq!(rows.row_count(), 2, "EU-WEST and EU-EAST match");
    assert_eq!(
        rows.columns()[1].cells,
        vec![Cell::Text("EU-WEST".into()), Cell::Text("EU-EAST".into())]
    );
}

#[test]
fn backspace_widens_the_filter_live() {
    let (mut app, _rx) = app_with_regions();
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu-w");
    assert_eq!(app.display_rows().unwrap().row_count(), 1);
    app.on_key(KeyEvent::plain(Key::Backspace), 300);
    app.on_key(KeyEvent::plain(Key::Backspace), 300);
    assert_eq!(app.display_rows().unwrap().row_count(), 2, "back to 'eu'");
}

#[test]
fn numeric_needle_matches_number_cells() {
    let (mut app, _rx) = app_with_regions();
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "2");
    let rows = app.display_rows().unwrap();
    assert_eq!(rows.row_count(), 1);
    assert_eq!(rows.columns()[0].cells, vec![Cell::Int(2)]);
}

#[test]
fn zero_match_needle_shows_no_rows_empty_state() {
    let (mut app, _rx) = app_with_regions();
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "zzz");
    assert_eq!(app.display_rows().unwrap().row_count(), 0);
    assert_eq!(app.empty_state(), Some("no rows match"));
}

#[test]
fn needle_edit_resets_vertical_scroll() {
    let (mut app, _rx) = app_with_regions();
    app.on_key(ctrl(Key::Char('t')), 300); // focus results
    app.on_key(KeyEvent::plain(Key::Down), 300); // scroll to offset 1
    assert_eq!(app.v_row_offset(), 1);
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    assert_eq!(app.v_row_offset(), 0, "filtered row set starts at the top");
}

// --- confirm / resume ---

#[test]
fn enter_confirms_and_navigation_resumes_over_filtered_rows() {
    let (mut app, _rx) = app_with_regions();
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    app.on_key(KeyEvent::plain(Key::Enter), 300);
    assert!(app.search().is_confirmed());
    assert_eq!(app.display_rows().unwrap().row_count(), 2);
    // Down now scrolls the (filtered) grid instead of editing the needle.
    app.on_key(KeyEvent::plain(Key::Down), 300);
    assert_eq!(app.v_row_offset(), 1);
    app.on_key(KeyEvent::plain(Key::Down), 300);
    assert_eq!(app.v_row_offset(), 1, "clamped to the 2 filtered rows");
    assert_eq!(app.search().needle(), "eu", "typing no longer edits it");
}

#[test]
fn enter_on_empty_needle_closes_instead_of_confirming() {
    let (mut app, _rx) = app_with_regions();
    app.on_key(ctrl(Key::Char('f')), 300);
    app.on_key(KeyEvent::plain(Key::Enter), 300);
    assert!(!app.search().is_visible(), "nothing to freeze");
}

#[test]
fn ctrl_f_on_confirmed_search_reenters_editing() {
    let (mut app, _rx) = app_with_regions();
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    app.on_key(KeyEvent::plain(Key::Enter), 300);
    app.on_key(ctrl(Key::Char('f')), 300);
    assert!(app.search().is_editing());
    assert_eq!(app.search().needle(), "eu", "the needle survives re-edit");
}

#[test]
fn esc_on_confirmed_search_clears_the_filter() {
    let (mut app, _rx) = app_with_regions();
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    app.on_key(KeyEvent::plain(Key::Enter), 300);
    app.on_key(KeyEvent::plain(Key::Esc), 300);
    assert!(!app.search().is_visible());
    assert_eq!(app.display_rows().unwrap().row_count(), 3);
}

// --- full-layout render (the bar row + the filtered grid together) ---

fn render(app: &App, w: u16, h: u16) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    t.backend().to_string()
}

#[test]
fn open_search_bar_renders_between_grid_and_query_box() {
    let (mut app, _rx) = app_with_regions();
    let before = render(&app, 60, 16);
    assert!(!before.contains("Search"), "no bar while closed:\n{before}");
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    let screen = render(&app, 60, 16);
    assert!(screen.contains("Search"), "bar title:\n{screen}");
    assert!(screen.contains("2/3 rows"), "filter badge:\n{screen}");
    assert!(
        screen.contains("EU-WEST") && screen.contains("EU-EAST"),
        "filtered rows drawn:\n{screen}"
    );
    assert!(
        !screen.contains("NA"),
        "the filtered-out row is gone:\n{screen}"
    );
}

#[test]
fn zero_match_search_renders_no_rows_match() {
    let (mut app, _rx) = app_with_regions();
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "zzz");
    let screen = render(&app, 60, 16);
    assert!(screen.contains("0/3 rows"), "{screen}");
    assert!(screen.contains("no rows match"), "{screen}");
}

// --- new results re-apply the filter ---

#[test]
fn new_result_reapplies_the_active_filter() {
    let (mut app, _rx) = app_with_regions();
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    app.on_key(KeyEvent::plain(Key::Enter), 300);
    assert_eq!(app.display_rows().unwrap().row_count(), 2);
    // A new query result lands (same shape, different rows).
    let table = Table::new(vec![
        Column::new("id", ColumnType::Int, vec![Cell::Int(9), Cell::Int(10)]),
        Column::new(
            "region",
            ColumnType::Text,
            vec![Cell::Text("EU-NORTH".into()), Cell::Text("SA".into())],
        ),
    ]);
    let schema = table.schema();
    app.on_key(ctrl(Key::Char('t')), 400); // focus back to the query bar
    type_str(&mut app, " WHERE 1=1", 400);
    app.tick(600);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: ProcessedResult::new(table, schema, 0),
        request_id: id,
        kind: RequestKind::Main,
    });
    let rows = app.display_rows().unwrap();
    assert_eq!(rows.row_count(), 1, "the standing needle filters new rows");
    assert_eq!(rows.columns()[1].cells, vec![Cell::Text("EU-NORTH".into())]);
}

// --- current-match navigation (n / N / Enter when confirmed) ---

/// An app whose result is `n` rows all containing "eu" in a `tag` column, plus a distinguishing
/// `id`. A prior render at 60x16 records the results-pane region so scrolloff math has a viewport.
fn app_with_many_eu_rows(n: usize) -> App {
    let (mut app, _rx) = loaded_app();
    type_str(&mut app, "SELECT * FROM t", 0);
    app.tick(150);
    let ids: Vec<Cell> = (0..n as i64).map(Cell::Int).collect();
    let tags: Vec<Cell> = (0..n).map(|i| Cell::Text(format!("eu-{i}"))).collect();
    let table = Table::new(vec![
        Column::new("id", ColumnType::Int, ids),
        Column::new("tag", ColumnType::Text, tags),
    ]);
    let schema = table.schema();
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: ProcessedResult::new(table, schema, 0),
        request_id: id,
        kind: RequestKind::Main,
    });
    if app.autocomplete().is_open() {
        app.on_key(KeyEvent::plain(Key::Esc), 200);
    }
    let _ = render(&app, 60, 16); // record the results-pane region for scrolloff
    app
}

#[test]
fn n_and_shift_n_navigate_matches_when_confirmed() {
    let mut app = app_with_many_eu_rows(30);
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    app.on_key(KeyEvent::plain(Key::Enter), 300); // confirm
    assert_eq!(
        app.search().current_row(),
        0,
        "first match current by default"
    );
    app.on_key(KeyEvent::char('n'), 300);
    assert_eq!(app.search().current_row(), 1);
    app.on_key(KeyEvent::char('n'), 300);
    assert_eq!(app.search().current_row(), 2);
    app.on_key(KeyEvent::char('N'), 300);
    assert_eq!(app.search().current_row(), 1, "Shift+N goes back");
}

#[test]
fn enter_when_confirmed_steps_to_next_match() {
    let mut app = app_with_many_eu_rows(10);
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    app.on_key(KeyEvent::plain(Key::Enter), 300); // confirm (current = 0)
    app.on_key(KeyEvent::plain(Key::Enter), 300); // next
    assert_eq!(app.search().current_row(), 1);
}

#[test]
fn navigation_wraps_at_the_ends() {
    let mut app = app_with_many_eu_rows(3);
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    app.on_key(KeyEvent::plain(Key::Enter), 300);
    app.on_key(KeyEvent::char('N'), 300); // 0 -> 2 (wrap back)
    assert_eq!(app.search().current_row(), 2);
    app.on_key(KeyEvent::char('n'), 300); // 2 -> 0 (wrap forward)
    assert_eq!(app.search().current_row(), 0);
}

#[test]
fn n_does_nothing_while_editing_or_unconfirmed() {
    let mut app = app_with_many_eu_rows(10);
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    // Still editing: 'n' is a needle char, not a navigation key.
    app.on_key(KeyEvent::char('n'), 300);
    assert_eq!(app.search().needle(), "eun");
    assert_eq!(app.search().current_row(), 0);
}

#[test]
fn navigating_down_scrolls_the_match_into_view_with_scrolloff() {
    let mut app = app_with_many_eu_rows(60);
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    app.on_key(KeyEvent::plain(Key::Enter), 300);
    // Step forward many times; the current match must stay within the visible body window, and
    // never be pinned to the very top/bottom edge until the data ends.
    for _ in 0..20 {
        app.on_key(KeyEvent::char('n'), 300);
    }
    let cur = app.search().current_row();
    let off = app.v_row_offset();
    let body_h = app.results_body_height() as usize;
    assert!(
        cur >= off && cur < off + body_h,
        "current match {cur} is inside the visible window [{off}, {})",
        off + body_h
    );
    assert!(
        cur > off,
        "with scrolloff, the match is not flush against the top edge"
    );
}

#[test]
fn navigating_to_a_right_edge_match_scrolls_the_grid_horizontally() {
    // A result whose ONLY matching cell is in a far-right column: navigating to that match must
    // scroll the grid right (h_col_offset advances) so the column comes into view.
    let (mut app, _rx) = loaded_app();
    type_str(&mut app, "SELECT * FROM t", 0);
    app.tick(150);
    // 8 wide text columns; the needle "zephyr" appears only in the last column of one row.
    let mut cols = Vec::new();
    for c in 0..8usize {
        let cells: Vec<Cell> = (0..3)
            .map(|r| {
                if c == 7 && r == 1 {
                    Cell::Text("zephyr-value".into())
                } else {
                    Cell::Text(format!("col{c}row{r}filler"))
                }
            })
            .collect();
        cols.push(Column::new(
            format!("column_{c:02}"),
            ColumnType::Text,
            cells,
        ));
    }
    let table = Table::new(cols);
    let schema = table.schema();
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: ProcessedResult::new(table, schema, 0),
        request_id: id,
        kind: RequestKind::Main,
    });
    if app.autocomplete().is_open() {
        app.on_key(KeyEvent::plain(Key::Esc), 200);
    }
    // Narrow viewport so the last column is definitely off the right edge at offset 0.
    let _ = render(&app, 40, 16);
    assert_eq!(app.h_col_offset(), 0, "starts scrolled fully left");
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "zephyr");
    app.on_key(KeyEvent::plain(Key::Enter), 300); // confirm; current match = the matching row
    assert!(
        app.h_col_offset() > 0,
        "navigating to the right-edge match scrolled the grid right (h_col_offset={})",
        app.h_col_offset()
    );
}

#[test]
fn last_match_may_reach_the_bottom_edge() {
    let mut app = app_with_many_eu_rows(60);
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    app.on_key(KeyEvent::plain(Key::Enter), 300);
    // Jump to the last match via one wrap-around N.
    app.on_key(KeyEvent::char('N'), 300);
    assert_eq!(app.search().current_row(), 59);
    let body_h = app.results_body_height() as usize;
    // The window is clamped at the data end: offset = row_count - body_h.
    assert_eq!(app.v_row_offset(), 60 - body_h);
}

#[test]
fn current_match_row_renders_in_the_distinct_style() {
    let mut app = app_with_many_eu_rows(5);
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    app.on_key(KeyEvent::plain(Key::Enter), 300); // confirm; current = row 0
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let mut t = Terminal::new(TestBackend::new(60, 16)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    let buf = t.backend().buffer().clone();
    // Row 0's "eu-0" cell should carry the current-match bg somewhere on screen.
    let cur_bg = crate::theme::grid::current_match().bg;
    let mut found = false;
    for y in 0..16u16 {
        for x in 0..60u16 {
            if buf[(x, y)].symbol() == "e" && buf[(x, y)].style().bg == cur_bg {
                found = true;
            }
        }
    }
    assert!(
        found,
        "the current match row paints in the current-match bg"
    );
}

#[test]
fn match_highlight_survives_vertical_scrolling() {
    // Regression: scrolling a confirmed filtered result must not drop the highlights on the rows
    // that scroll into view.
    let mut app = app_with_many_eu_rows(60);
    app.on_key(ctrl(Key::Char('f')), 300);
    type_needle(&mut app, "eu");
    app.on_key(KeyEvent::plain(Key::Enter), 300);
    for _ in 0..15 {
        app.on_key(KeyEvent::char('n'), 300);
    }
    assert!(app.v_row_offset() > 0, "we actually scrolled");
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let mut t = Terminal::new(TestBackend::new(60, 16)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    let buf = t.backend().buffer().clone();
    let match_bg = crate::theme::grid::search_match().bg;
    let cur_bg = crate::theme::grid::current_match().bg;
    let mut highlighted = 0;
    for y in 0..16u16 {
        for x in 0..60u16 {
            let bg = buf[(x, y)].style().bg;
            if bg == match_bg || bg == cur_bg {
                highlighted += 1;
            }
        }
    }
    assert!(
        highlighted > 0,
        "scrolled-in rows still carry match highlights"
    );
}
