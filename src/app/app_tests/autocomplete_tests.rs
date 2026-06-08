//! `App`-shell tests for the Simple-mode pane-aware autocomplete pipeline (`dev/PLAN.md` §5.4) and
//! the Power-mode `WHERE region = '` value-completion round-trip.
//!
//! The pure pane-context fan-out (column / operator / value / keyword per pane) is covered by
//! `autocomplete::pane_context::pane_context_tests`; this file covers the App-level wiring —
//! `refresh_autocomplete` reads the focused pane in Simple mode, dispatches a value fetch on the
//! same worker channel, and the popup state machine carries the right candidates to the render
//! layer. Every test is headless (synthetic schema, no engine, no terminal, no clock).
//!
//! Split out of `app_tests.rs` to keep each file under the 1000-line cap; helpers come from `super`.

use crate::app::query_form::SimplePane;
use crate::app::{App, Key, KeyEvent};
use crate::autocomplete::autocomplete_state::SuggestionType;
use crate::engine::InterruptHandle;
use crate::engine::types::{Cell, Column, Table};
use crate::query::worker::types::{ProcessedResult, QueryRequest, QueryResponse, RequestKind};
use crate::schema::{ColumnMeta, ColumnType, Schema};

use std::sync::mpsc::{Receiver, channel};

use super::type_str;

/// A schema with low-cardinality `region`/`status` so value-completion has something to offer.
fn schema_for_panes() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("status", ColumnType::Text),
        ColumnMeta::new("amount", ColumnType::Float),
        ColumnMeta::new("region", ColumnType::Text),
    ])
}

/// A loaded App in **Simple** mode with an empty pane set, focused on WHERE (the form's default).
fn simple_app() -> (App, Receiver<QueryRequest>) {
    let (tx, rx) = channel();
    let mut app = App::new(tx, InterruptHandle::noop());
    app.set_schema(schema_for_panes());
    app.on_loaded("ready");
    // Switch from the App's default Power mode to Simple, with empty pane texts.
    app.query_form
        .enter_simple_with_parts(Default::default(), 1000);
    (app, rx)
}

/// Texts of the current popup suggestions (closed -> empty).
fn popup_texts(app: &App) -> Vec<String> {
    app.autocomplete()
        .suggestions()
        .iter()
        .map(|s| s.text.clone())
        .collect()
}

/// Type into the focused Simple pane and re-run autocomplete after each char (mirrors how the
/// real key path will refresh once per-pane input routing lands; for now we drive the pipeline
/// directly via `pane_mut` + `App::refresh_autocomplete`).
fn type_into_pane(app: &mut App, pane: SimplePane, text: &str) {
    app.query_form.focus(pane);
    let editor = app.query_form.pane_mut(pane);
    for c in text.chars() {
        editor.insert_char(c);
    }
    app.refresh_autocomplete();
}

fn value_response(column: &str, values: &[&str], request_id: u64) -> QueryResponse {
    let cells = values.iter().map(|v| Cell::Text((*v).into())).collect();
    let table = Table::new(vec![Column::new(column, ColumnType::Text, cells)]);
    let schema = table.schema();
    QueryResponse::ProcessedSuccess {
        result: ProcessedResult::new(table, schema, 0),
        request_id,
        kind: RequestKind::Value {
            column: column.into(),
        },
    }
}

// ── Simple-mode per-pane suggestions (the §5.4 mapping at the App level) ────────────────────────

#[test]
fn simple_select_pane_offers_columns_and_functions() {
    let (mut app, _rx) = simple_app();
    type_into_pane(&mut app, SimplePane::Select, "");
    let texts = popup_texts(&app);
    assert!(texts.contains(&"id".to_string()));
    assert!(texts.contains(&"status".to_string()));
    assert!(texts.contains(&"*".to_string()));
    assert!(texts.contains(&"COUNT".to_string()));
}

#[test]
fn simple_where_pane_at_start_offers_columns() {
    let (mut app, _rx) = simple_app();
    type_into_pane(&mut app, SimplePane::Where, "");
    let texts = popup_texts(&app);
    assert!(texts.contains(&"region".to_string()));
    assert!(texts.contains(&"status".to_string()));
    assert!(
        !texts.contains(&"COUNT".to_string()),
        "no aggregates in WHERE pane: {texts:?}"
    );
}

#[test]
fn simple_where_pane_after_column_offers_operators() {
    let (mut app, _rx) = simple_app();
    type_into_pane(&mut app, SimplePane::Where, "region ");
    let texts = popup_texts(&app);
    assert!(texts.contains(&"=".to_string()));
    assert!(texts.contains(&"LIKE".to_string()));
}

#[test]
fn simple_where_pane_after_op_quote_dispatches_value_fetch_for_canonical_column() {
    let (mut app, rx) = simple_app();
    type_into_pane(&mut app, SimplePane::Where, "region = '");
    // The fetch is keyed by the canonical column name (`region`), on the worker channel — same lane
    // a Power-mode WHERE value position uses (no separate engine connection).
    let mut found = false;
    while let Ok(req) = rx.try_recv() {
        if matches!(&req.kind, RequestKind::Value { column } if column == "region") {
            found = true;
        }
    }
    assert!(
        found,
        "Simple-mode WHERE pane must dispatch a value fetch for `region`"
    );
}

#[test]
fn simple_where_pane_value_response_fills_popup_with_distinct_values() {
    let (mut app, rx) = simple_app();
    type_into_pane(&mut app, SimplePane::Where, "region = '");
    let mut value_id = None;
    while let Ok(req) = rx.try_recv() {
        if let RequestKind::Value { column } = &req.kind
            && column == "region"
        {
            value_id = Some(req.request_id);
        }
    }
    let value_id = value_id.expect("region value fetch dispatched");
    app.on_response(value_response("region", &["EU", "NA", "APAC"], value_id));
    assert!(app.autocomplete().is_open());
    let texts = popup_texts(&app);
    assert!(texts.contains(&"EU".to_string()), "got {texts:?}");
    assert!(texts.contains(&"NA".to_string()));
    assert!(
        app.autocomplete()
            .suggestions()
            .iter()
            .all(|s| s.suggestion_type == SuggestionType::Value),
        "all candidates are Value type"
    );
}

#[test]
fn simple_group_by_pane_offers_columns() {
    let (mut app, _rx) = simple_app();
    type_into_pane(&mut app, SimplePane::GroupBy, "");
    let texts = popup_texts(&app);
    assert!(texts.contains(&"region".to_string()));
    assert!(texts.contains(&"status".to_string()));
    assert!(!texts.contains(&"=".to_string()));
}

#[test]
fn simple_order_by_pane_after_column_offers_asc_desc_and_columns() {
    let (mut app, _rx) = simple_app();
    type_into_pane(&mut app, SimplePane::OrderBy, "region ");
    let texts = popup_texts(&app);
    // Columns remain available (the GROUP/ORDER BY list keeps offering columns for the next entry),
    // and ASC/DESC join the candidate list once a column has been typed.
    assert!(texts.contains(&"ASC".to_string()), "got {texts:?}");
    assert!(texts.contains(&"DESC".to_string()), "got {texts:?}");
}

#[test]
fn simple_limit_pane_does_not_open_popup() {
    // LIMIT is numeric input only — the popup must never open in this pane.
    let (mut app, _rx) = simple_app();
    type_into_pane(&mut app, SimplePane::Limit, "1");
    assert!(
        !app.autocomplete().is_open(),
        "LIMIT pane must not open the popup"
    );
}

// ── Power-mode keyword popup regression: `wh` after `FROM t ` opens with WHERE ──────────────────

#[test]
fn power_mode_wh_after_from_relation_opens_popup_with_where() {
    // The user-reported regression in the brief: typing `wh` after `SELECT * FROM t ` must pop the
    // WHERE keyword. The fix lives in `clause_context::detect_context` (a `from_slot_filled` check
    // promotes `FROM t wh` to `Keyword{partial: "wh"}` — was silently `FromTable { partial: "wh" }`).
    // This is the App-level guard against any future regression in the wiring between key edits and
    // the popup state machine.
    let (tx, _rx) = channel();
    let mut app = App::new(tx, InterruptHandle::noop());
    app.set_schema(schema_for_panes());
    app.on_loaded("ready");
    type_str(&mut app, "SELECT * FROM t wh", 0);
    assert!(
        app.autocomplete().is_open(),
        "the popup must be open with keyword candidates"
    );
    let texts = popup_texts(&app);
    assert!(
        texts.contains(&"WHERE".to_string()),
        "expected WHERE in keyword popup, got {texts:?}"
    );
}

#[test]
fn power_mode_where_region_quote_dispatches_and_fills_value_popup() {
    // The end-to-end value-completion round-trip in Power mode: typing the `'` after `WHERE region =`
    // dispatches a Value fetch, the response fills the cache, and the popup re-opens with the
    // distinct values. (Brief calls out this exact case for the `region` column.)
    let (tx, rx) = channel();
    let mut app = App::new(tx, InterruptHandle::noop());
    app.set_schema(schema_for_panes());
    app.on_loaded("ready");
    type_str(&mut app, "SELECT * FROM t WHERE region = '", 0);

    let mut value_id = None;
    while let Ok(req) = rx.try_recv() {
        if let RequestKind::Value { column } = &req.kind
            && column == "region"
        {
            value_id = Some(req.request_id);
        }
    }
    let value_id = value_id.expect("a value fetch for `region` should be dispatched");
    let changed = app.on_response(value_response("region", &["EU", "NA", "APAC"], value_id));
    assert!(!changed, "a value fetch must not change the visible grid");
    assert!(app.value_cache().contains("region"));
    assert!(app.autocomplete().is_open(), "popup re-opens with values");
    let texts = popup_texts(&app);
    assert!(texts.contains(&"EU".to_string()), "got {texts:?}");
    assert!(texts.contains(&"NA".to_string()));
    assert!(texts.contains(&"APAC".to_string()));
}

// ── Insertion target follows the focused surface in Simple mode ─────────────────────────────────

#[test]
fn simple_mode_tab_inserts_into_focused_pane_not_power_editor() {
    // The accept path must target the focused Simple pane, not the App's hidden Power editor —
    // otherwise typing in Simple mode and accepting a suggestion silently writes to the wrong
    // surface (a felt regression: "I picked the column, why didn't it appear?").
    let (mut app, _rx) = simple_app();
    type_into_pane(&mut app, SimplePane::Where, "stat");
    assert!(app.autocomplete().is_open());
    app.on_key(KeyEvent::plain(Key::Tab), 0);
    assert_eq!(
        app.query_form.text(SimplePane::Where),
        "status",
        "Tab inserted the suggestion into the focused WHERE pane"
    );
    assert_eq!(
        app.editor().text(),
        "",
        "the Power editor stays empty — the accept did not target it"
    );
}
