//! App — the TUI shell state, event routing, and the felt query loop.
//!
//! `dev/PLAN.md` §3 (architecture / data flow) + §0/D4 (out-of-band cancel). The `App` is the
//! engine-ignorant core of the shell: it owns the query-bar [`Editor`], the [`Debouncer`], the
//! [`Dispatcher`] (request `Sender` + `InterruptHandle` clone + stale-discard `QueryState`), the
//! latest result + scroll offsets, and the load state machine ([`AppPhase`]). It talks to the
//! worker only through `QueryRequest`/`QueryResponse` over channels — never to DuckDB directly.
//!
//! Everything here is **headless**: synthetic [`KeyEvent`]s route through [`App::on_key`], time
//! enters through `now_ms: u64` parameters (never a wall clock), and the render path
//! ([`app_render`]) is a pure function of `App` state into a `Frame`, so `TestBackend`
//! exercises it fully. The only terminal edge is the thin crossterm loop in [`event_loop`],
//! which is the §4.7 human-validated surface.
//!
//! ## The felt loop (per §3.1)
//!
//! keystroke -> [`Editor`] mutation -> [`Debouncer::schedule_execution_at`]; on a debounce fire
//! ([`App::tick`]) -> [`prepare_interactive`](crate::query::preprocess::prepare_interactive)
//! validate + LIMIT-wrap -> [`Dispatcher::dispatch`] (interrupts any prior in-flight query) ->
//! worker runs -> [`App::on_response`] -> [`Dispatcher::accept`] stale-discard -> update result
//! -> re-render. Invalid SQL (preprocess reject or `QueryResponse::Error`) -> a status-line
//! error via [`enhance`](crate::query::error_enhance::enhance), never a crash.
//!
//! ## Load state machine (P2.11)
//!
//! `Loading -> Ready -> Querying`, plus `LoadError`. The CSV parse happens off the UI thread (a
//! loader thread; see [`event_loop`]); the query bar is **editable during load** and a query
//! typed while `Loading` fires once the engine reaches `Ready` ([`App::on_loaded`]).

use ratatui::Frame;

use crate::autocomplete::autocomplete_state::AutocompleteState;
use crate::autocomplete::candidates::get_suggestions;
use crate::autocomplete::clause_context::{CursorContext, detect_context};
use crate::autocomplete::insertion::insert_suggestion;
use crate::autocomplete::sql_keywords::OPERATORS;
use crate::autocomplete::value_source::{ValueCache, build_distinct_sql_default};
use crate::engine::InterruptHandle;
use crate::palette::PaletteState;
use crate::palette::query_emit::emit as emit_palette;
use crate::query::debouncer::Debouncer;
use crate::query::dispatcher::Dispatcher;
use crate::query::error_enhance::enhance;
use crate::query::preprocess::{PreprocessError, prepare_interactive};
use crate::query::worker::types::{ProcessedResult, QueryRequest, QueryResponse, RequestKind};
use crate::schema::Schema;
use crate::sql_lexer::tokenize;

use std::sync::mpsc::Sender;

pub mod app_render;
pub mod editor;
pub mod event_loop;
pub mod key;

pub use editor::Editor;
pub use key::{Key, KeyEvent, KeyMods};

/// How many rows the LIMIT-wrap caps an interactive query to. A screenful-plus-margin so a
/// bare `SELECT *` returns a viewport, not the whole table (the §2.3 latency guard). The grid
/// only ever paints the visible window; the rest is scroll headroom.
pub const VIEWPORT_ROW_LIMIT: usize = 1000;

/// The load/query lifecycle (`dev/PLAN.md` §3, P2.11).
///
/// `Loading` is the one-time parse-once state; `Ready` is loaded and idle; `Querying` means a
/// debounced query is in-flight; `LoadError` is a terminal failure to ingest the CSV. Editing
/// the query bar is allowed in every state except `LoadError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppPhase {
    /// Parsing the CSV once at startup (off the UI thread). Query bar is editable.
    Loading,
    /// Loaded and idle, ready for queries.
    Ready,
    /// A debounced query has been dispatched and is in-flight.
    Querying,
    /// The CSV could not be loaded. Terminal state; carries the message for the status line.
    LoadError(String),
}

/// Which surface currently has keyboard focus. Minimal in Phase 2 (only the query bar exists);
/// the results pane and popups become focus targets in later phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    /// The query bar receives text edits; arrow keys move the cursor.
    #[default]
    QueryBar,
    /// The results grid receives navigation (scroll) keys.
    Results,
}

/// The TUI application state (engine-ignorant; talks to the worker over channels only).
pub struct App {
    phase: AppPhase,
    focus: Focus,
    editor: Editor,
    debouncer: Debouncer,
    dispatcher: Dispatcher,
    /// The latest accepted successful result, if any (None until the first query lands).
    result: Option<ProcessedResult>,
    /// Vertical row scroll offset into the result body.
    v_row_offset: usize,
    /// Horizontal column scroll offset (column-granular).
    h_col_offset: usize,
    /// The status-line text shown at the bottom.
    status: String,
    /// Whether the user edited the query while still `Loading`, so we must fire once `Ready`.
    pending_query_on_ready: bool,
    /// The loaded table schema — the candidate source for autocomplete. `None` until the engine
    /// reaches `Ready` ([`on_loaded`](Self::on_loaded) installs it). Without it the popup stays
    /// closed (no schema = no column/value candidates to ground completion in).
    schema: Option<Schema>,
    /// The autocomplete popup state machine (P3.6) — recomputed on each query-bar edit.
    autocomplete: AutocompleteState,
    /// Distinct-value cache for value-completion (P3.7), filled out-of-band by the worker; read as
    /// plain data by the candidate generator. The App never queries the engine for values itself.
    value_cache: ValueCache,
    /// The active CSV dialect summary shown in the schema bar (P4.1): the effective delimiter
    /// (`None` = DuckDB auto-detected it) and whether the first row is a header. Defaults to
    /// `(None, true)` until the loader reports the dialect ([`set_csv_summary`](Self::set_csv_summary)).
    csv_summary: (Option<char>, bool),
    /// The column-palette generated-state machine (P4.2-P4.5, §0/D3). Built from the schema on
    /// load ([`set_schema`](Self::set_schema)); `None` until then. The palette owns a ciq-generated
    /// query state and emits SQL from it — it never parses the bar. Ownership ("is the palette
    /// live?") is a byte-compare of the bar against the last string the palette emitted
    /// ([`palette_owns_query`](Self::palette_owns_query)); the App never inspects user SQL grammar.
    palette: Option<PaletteState>,
    /// Whether the palette popup is currently open (P4.5). When open it intercepts keys (toggle /
    /// reorder / filter / emit) before the query-bar routing, like the autocomplete popup.
    palette_open: bool,
}

impl App {
    /// Build an `App` over the dispatcher's request channel + interrupt handle. Starts in
    /// `Loading` (the engine is ingesting off-thread).
    pub fn new(request_tx: Sender<QueryRequest>, interrupt: InterruptHandle) -> Self {
        Self {
            phase: AppPhase::Loading,
            focus: Focus::QueryBar,
            editor: Editor::new(),
            debouncer: Debouncer::new(),
            dispatcher: Dispatcher::new(request_tx, interrupt),
            result: None,
            v_row_offset: 0,
            h_col_offset: 0,
            status: "loading CSV…".to_string(),
            pending_query_on_ready: false,
            schema: None,
            autocomplete: AutocompleteState::new(),
            value_cache: ValueCache::new(),
            csv_summary: (None, true),
            palette: None,
            palette_open: false,
        }
    }

    // --- read-only accessors (tests assert on structured state, not just the screen) ---

    pub fn phase(&self) -> &AppPhase {
        &self.phase
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    pub fn query(&self) -> &str {
        self.editor.text()
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn result(&self) -> Option<&ProcessedResult> {
        self.result.as_ref()
    }

    pub fn v_row_offset(&self) -> usize {
        self.v_row_offset
    }

    pub fn h_col_offset(&self) -> usize {
        self.h_col_offset
    }

    /// The most recently issued query's `request_id` (0 before any dispatch).
    pub fn latest_request_id(&self) -> u64 {
        self.dispatcher.latest_id()
    }

    /// Whether the dispatcher believes a main query is in-flight (the D4 interrupt gate). Read-only;
    /// exposed so tests can assert a value-lane response never desyncs this bookkeeping.
    pub fn is_query_in_flight(&self) -> bool {
        self.dispatcher.in_flight()
    }

    /// The autocomplete popup state (for the render layer and tests).
    pub fn autocomplete(&self) -> &AutocompleteState {
        &self.autocomplete
    }

    /// The loaded schema, if the engine has reached `Ready`.
    pub fn schema(&self) -> Option<&Schema> {
        self.schema.as_ref()
    }

    /// The distinct-value cache (for tests asserting value-completion wiring).
    pub fn value_cache(&self) -> &ValueCache {
        &self.value_cache
    }

    /// The active CSV dialect for the schema bar: the effective delimiter (`None` = auto-detected)
    /// and whether the first row is a header.
    pub fn csv_summary(&self) -> (Option<char>, bool) {
        self.csv_summary
    }

    // --- event routing (headless: synthetic KeyEvents) ---

    /// Route one key event to the focused surface, scheduling a debounced query when the query
    /// text changes. `now_ms` is the synthetic/real time stamp for the debouncer (never a wall
    /// clock read inside this fn). Returns `true` if the app should quit.
    ///
    /// When the autocomplete popup is open it intercepts Tab/Enter (accept), Up/Down (move
    /// selection), and Esc (dismiss) *before* the focus routing — so Esc dismisses the popup
    /// rather than quitting (it only quits when no popup is open).
    pub fn on_key(&mut self, ev: KeyEvent, now_ms: u64) -> bool {
        if self.autocomplete.is_open() && self.handle_popup_key(&ev, now_ms) {
            return false; // the popup consumed the key
        }
        // Ctrl-C / Esc quit from anywhere (Esc only reaches here when no popup is open).
        if ev.is_quit() {
            return true;
        }
        match self.focus {
            Focus::QueryBar => self.on_key_query_bar(ev, now_ms),
            Focus::Results => self.on_key_results(ev),
        }
        false
    }

    /// Handle a key while the popup is open. Returns `true` if the popup consumed it (so the
    /// caller stops routing). Tab/Enter accept the selection; Up/Down move it; Esc dismisses; any
    /// other key falls through (e.g. typing keeps editing and recomputes the popup).
    fn handle_popup_key(&mut self, ev: &KeyEvent, now_ms: u64) -> bool {
        match ev.key {
            Key::Tab | Key::Enter => {
                self.accept_suggestion(now_ms);
                true
            }
            Key::Down => {
                self.autocomplete.select_next();
                true
            }
            Key::Up => {
                self.autocomplete.select_prev();
                true
            }
            Key::Esc => {
                self.autocomplete.close();
                true
            }
            _ => false,
        }
    }

    /// Insert the selected suggestion into the query at the cursor and dismiss the popup. The
    /// popup stays closed after an explicit accept (it does not re-open on the just-completed
    /// token); the next edit recomputes it for the new context. Closes without inserting if there
    /// is nothing selected.
    fn accept_suggestion(&mut self, now_ms: u64) {
        let Some(suggestion) = self.autocomplete.selected_suggestion().cloned() else {
            self.autocomplete.close();
            return;
        };
        let (new_text, new_cursor) =
            insert_suggestion(self.editor.text(), self.editor.cursor_byte(), &suggestion);
        self.editor.set_text_with_byte_cursor(new_text, new_cursor);
        self.autocomplete.close();
        // The inserted text changed the query — schedule the debounced grid query for it.
        self.schedule(now_ms);
    }

    fn on_key_query_bar(&mut self, ev: KeyEvent, now_ms: u64) {
        if matches!(self.phase, AppPhase::LoadError(_)) {
            return; // bar is frozen once load failed
        }
        let before = self.editor.text().to_string();
        match ev.key {
            Key::Char(c) => self.editor.insert_char(c),
            Key::Backspace => self.editor.backspace(),
            Key::Delete => self.editor.delete(),
            Key::Left => self.editor.move_left(),
            Key::Right => self.editor.move_right(),
            Key::Home => self.editor.move_home(),
            Key::End => self.editor.move_end(),
            Key::Paste(ref s) => self.editor.insert_str(s),
            Key::Down => self.focus = Focus::Results, // hand off navigation to the grid
            _ => {}
        }
        // Recompute autocomplete on any edit/cursor move (the popup tracks the cursor context).
        self.refresh_autocomplete();
        // Only (re)schedule a query when the text actually changed — pure cursor moves don't.
        if self.editor.text() != before {
            self.schedule(now_ms);
        }
    }

    /// Recompute the autocomplete popup from the current query + cursor against the loaded schema,
    /// and (P3.7) fetch distinct values through the worker when the cursor is in a value position
    /// for a column not yet cached. Closes the popup when there is no schema (still loading) or no
    /// candidate applies. Pure except for the out-of-band value fetch.
    fn refresh_autocomplete(&mut self) {
        let Some(schema) = self.schema.as_ref() else {
            self.autocomplete.close();
            return;
        };
        let query = self.editor.text();
        let cursor = self.editor.cursor_byte();

        // If the cursor is in a value position for an uncached, known column, fetch its distinct
        // values through the worker (same channel/engine — autocomplete never opens its own
        // connection, §5.5). The popup fills in once the response lands.
        if let Some(col) = self.value_column_to_fetch(query, cursor, schema) {
            let sql = build_distinct_sql_default(&col);
            let _ = self.dispatcher.dispatch_value(sql, col);
        }

        let suggestions = get_suggestions(query, cursor, schema, OPERATORS, &self.value_cache);
        self.autocomplete.open_with(suggestions);
    }

    /// The column whose distinct values should be fetched now: `Some(canonical_name)` when the
    /// cursor is in a `ColumnValue` context for a column present in `schema` and not already cached;
    /// `None` otherwise (no value position, unknown column, or already cached).
    ///
    /// The detected column text keeps the user's casing (`STATUS`), but DuckDB resolves unquoted
    /// identifiers case-insensitively, so we resolve to the canonical header spelling (`status`)
    /// and key the fetch + cache by it — keeping the fetch key, the cache key, and the candidate
    /// generator's lookup all in lockstep (see [`Schema::column_ci`]).
    fn value_column_to_fetch(&self, query: &str, cursor: usize, schema: &Schema) -> Option<String> {
        let tokens = tokenize(query);
        let CursorContext::ColumnValue { col, .. } = detect_context(query, &tokens, cursor) else {
            return None;
        };
        let canonical = &schema.column_ci(&col)?.name;
        if self.value_cache.contains(canonical) {
            None
        } else {
            Some(canonical.clone())
        }
    }

    fn on_key_results(&mut self, ev: KeyEvent) {
        match ev.key {
            Key::Up if self.v_row_offset == 0 => self.focus = Focus::QueryBar,
            Key::Up => self.v_row_offset = self.v_row_offset.saturating_sub(1),
            Key::Down => self.scroll_down(1),
            Key::PageUp => self.v_row_offset = self.v_row_offset.saturating_sub(10),
            Key::PageDown => self.scroll_down(10),
            Key::Left => self.h_col_offset = self.h_col_offset.saturating_sub(1),
            Key::Right => self.scroll_right(),
            Key::Home => self.v_row_offset = 0,
            _ => {}
        }
    }

    fn scroll_down(&mut self, by: usize) {
        let row_count = self
            .result
            .as_ref()
            .map(|r| r.rows.row_count())
            .unwrap_or(0);
        let max = row_count.saturating_sub(1);
        self.v_row_offset = (self.v_row_offset + by).min(max);
    }

    fn scroll_right(&mut self) {
        let col_count = self
            .result
            .as_ref()
            .map(|r| r.rows.col_count())
            .unwrap_or(0);
        let max = col_count.saturating_sub(1);
        self.h_col_offset = (self.h_col_offset + 1).min(max);
    }

    /// (Re)schedule a debounced query at `now_ms`. While `Loading` we only remember that a query
    /// is pending (it can't run until the engine is `Ready`); the debounce window still gates it.
    fn schedule(&mut self, now_ms: u64) {
        self.debouncer.schedule_execution_at(now_ms);
        if matches!(self.phase, AppPhase::Loading) {
            self.pending_query_on_ready = true;
        }
    }

    // --- the debounce fire edge ---

    /// Drive the debouncer at `now_ms`: if the quiet window has elapsed and the engine is
    /// loaded, dispatch the current query (preprocessed + LIMIT-wrapped). Returns `true` if a
    /// query was dispatched. Called once per event-loop turn with the current time.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        if !self.debouncer.should_execute_at(now_ms) {
            return false;
        }
        // The engine isn't ready yet; keep the pending flag set so on_loaded fires it. The
        // debounce window has already elapsed, so it fires immediately on Ready.
        if matches!(self.phase, AppPhase::Loading | AppPhase::LoadError(_)) {
            return false;
        }
        self.debouncer.mark_executed();
        self.dispatch_current()
    }

    /// Preprocess + dispatch the current query text. Empty input clears the result; a rejected
    /// grammar sets a status-line error and issues no engine call.
    fn dispatch_current(&mut self) -> bool {
        let raw = self.editor.text().trim();
        if raw.is_empty() {
            self.result = None;
            self.v_row_offset = 0;
            self.h_col_offset = 0;
            self.status = "ready".to_string();
            self.phase = AppPhase::Ready;
            return false;
        }
        match prepare_interactive(raw, VIEWPORT_ROW_LIMIT) {
            Ok(sql) => match self.dispatcher.dispatch(sql) {
                Ok(_id) => {
                    self.phase = AppPhase::Querying;
                    self.status = "running…".to_string();
                    true
                }
                Err(_) => {
                    // Worker gone — surface it without crashing.
                    self.status = "worker unavailable".to_string();
                    false
                }
            },
            Err(e) => {
                self.set_query_error(e);
                false
            }
        }
    }

    fn set_query_error(&mut self, e: PreprocessError) {
        self.status = e.message().to_string();
        // Stay Ready (the bar is still live) but show no stale grid for an invalid query.
        self.phase = AppPhase::Ready;
    }

    // --- response handling (stale-discard + state update) ---

    /// Apply a [`QueryResponse`] from the worker. A **value-completion** response (P3.7) is routed
    /// to the [`ValueCache`] and never touches the grid; a **main** response goes through the
    /// stale-discard gate (§0/D4) — an older `request_id` is dropped before touching result state.
    /// A per-request engine panic arrives as `Error` under that query's id and is handled the same
    /// way. Returns `true` if the visible state changed.
    pub fn on_response(&mut self, resp: QueryResponse) -> bool {
        // Value fetches are out of the main request lane: route them to the cache without the
        // stale-discard gate (their ids live in a separate value lane), then refresh the popup so
        // the just-fetched values appear.
        if let QueryResponse::ProcessedSuccess {
            result,
            kind: RequestKind::Value { column },
            ..
        } = &resp
        {
            let values = distinct_values(&result.rows);
            self.value_cache.insert(column.clone(), values);
            self.refresh_autocomplete();
            return false; // value cache filled; the grid is unchanged
        }
        if let QueryResponse::Error {
            kind: RequestKind::Value { .. },
            ..
        } = &resp
        {
            return false; // a failed value fetch silently yields no candidates
        }
        // A cancelled value fetch is also out of the main lane: drop it *before* the stale-discard
        // gate. Otherwise its value-lane id, drawn from a counter that overlaps the main `latest_id`,
        // could collide and wrongly clear `in_flight` while a real main query is still running (D4).
        if let QueryResponse::Cancelled {
            kind: RequestKind::Value { .. },
            ..
        } = &resp
        {
            return false; // the value popup simply gets no candidates this round
        }

        let id = resp.request_id();
        if !self.dispatcher.accept(id) {
            return false; // stale: a newer query superseded this one
        }
        match resp {
            QueryResponse::ProcessedSuccess { result, .. } => {
                let rows = result.rows.row_count();
                self.status = format!("{rows} row{}", if rows == 1 { "" } else { "s" });
                self.result = Some(result);
                self.v_row_offset = 0;
                self.phase = AppPhase::Ready;
                true
            }
            QueryResponse::Error { message, .. } => {
                self.status = enhance(&message);
                self.phase = AppPhase::Ready;
                true
            }
            QueryResponse::Cancelled { .. } => false, // superseded; nothing to show
        }
    }

    /// Install the real engine interrupt handle once load completes (the shell starts with a
    /// no-op placeholder — see [`Dispatcher::set_interrupt`]).
    pub fn set_interrupt(&mut self, interrupt: InterruptHandle) {
        self.dispatcher.set_interrupt(interrupt);
    }

    /// Install the loaded table schema (the autocomplete candidate source) and build the column
    /// palette over it (P4.2/D3). The event loop calls this on load completion with the engine's
    /// schema, before [`on_loaded`](Self::on_loaded). Until it is set, the autocomplete popup stays
    /// closed and the palette is unavailable (no schema = no columns to pick).
    pub fn set_schema(&mut self, schema: Schema) {
        self.palette = Some(PaletteState::from_schema(&schema));
        self.schema = Some(schema);
    }

    /// Install the active CSV dialect for the schema bar (P4.1): the effective delimiter (`None`
    /// when DuckDB auto-detected it) and whether the first row is a header. Set by the event loop
    /// from the launch [`CsvOpts`](crate::engine::CsvOpts) alongside [`set_schema`](Self::set_schema).
    pub fn set_csv_summary(&mut self, delimiter: Option<char>, header: bool) {
        self.csv_summary = (delimiter, header);
    }

    // --- column palette (P4.2-P4.5, §0/D3) ---

    /// The column palette, if a schema is loaded.
    pub fn palette(&self) -> Option<&PaletteState> {
        self.palette.as_ref()
    }

    /// Whether the palette popup is currently open.
    pub fn is_palette_open(&self) -> bool {
        self.palette_open
    }

    /// Whether the **palette owns the current query** — i.e. the bar text byte-equals the last
    /// string the palette emitted (§0/D3). Equal -> the palette's edits stay live; different (or no
    /// palette / never emitted) -> the user hand-typed SQL and palette actions must offer a soft
    /// "Replace?" rather than silently rewriting. A pure byte-compare; **no SQL parsing** anywhere.
    pub fn palette_owns_query(&self) -> bool {
        self.palette
            .as_ref()
            .is_some_and(|p| p.owns(self.editor.text()))
    }

    /// Pre-seed the query bar with the palette's own emission (`SELECT * FROM t LIMIT n`) so the
    /// common path — open a file, no SQL typed yet — starts **palette-owned** (§0/D3). Only seeds
    /// when a palette exists and the bar is still empty (the user typed nothing during load); it
    /// never clobbers a query the user already started. Records the emitted string as the palette's
    /// own and schedules the query so the grid populates. The event loop calls this once after
    /// load. Returns `true` if it seeded.
    pub fn seed_palette_query(&mut self, now_ms: u64) -> bool {
        if !self.editor.text().is_empty() {
            return false;
        }
        let Some(palette) = self.palette.as_mut() else {
            return false;
        };
        let sql = emit_palette(palette);
        palette.record_emitted(&sql);
        self.editor.set_text(&sql);
        self.refresh_autocomplete();
        self.schedule(now_ms);
        true
    }

    /// Re-emit the palette's query and replace the bar with it (the "Replace query with column
    /// selection?" affordance, §0/D3). This is the **only** path that overwrites hand-typed SQL,
    /// and only on explicit user confirmation — accepting Replace discards whatever the user typed
    /// and snaps to the palette's generated query (the documented UX cliff: a hand-typed
    /// `… WHERE region='EU'` is discarded). Records the new emission as palette-owned and schedules
    /// it. No-op without a palette. Returns the emitted SQL it installed.
    pub fn replace_query_with_palette(&mut self, now_ms: u64) -> Option<String> {
        let palette = self.palette.as_mut()?;
        let sql = emit_palette(palette);
        palette.record_emitted(&sql);
        self.editor.set_text(&sql);
        self.refresh_autocomplete();
        self.schedule(now_ms);
        Some(sql)
    }

    // --- load state machine (P2.11) ---

    /// The engine finished loading (`row_count` rows ingested). Transition `Loading -> Ready`,
    /// and if the user typed a query during load, dispatch it now (the debounce window already
    /// elapsed while loading). Returns `true` if a query was dispatched on becoming ready.
    pub fn on_loaded(&mut self, status: impl Into<String>) -> bool {
        self.phase = AppPhase::Ready;
        self.status = status.into();
        if self.pending_query_on_ready {
            self.pending_query_on_ready = false;
            self.debouncer.mark_executed();
            return self.dispatch_current();
        }
        false
    }

    /// The engine failed to load the CSV. Terminal `LoadError` state; the message is shown in
    /// the status line and the query bar is frozen.
    pub fn on_load_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.status = format!("load error: {message}");
        self.phase = AppPhase::LoadError(message);
    }

    /// Render the current state into `frame` (delegates to [`app_render`]). Pure function of
    /// `App` state — no terminal, no clock, no I/O.
    pub fn render(&self, frame: &mut Frame) {
        app_render::render(self, frame);
    }
}

/// Extract the distinct value strings from a value-fetch result table — the **first** column (the
/// `build_distinct_sql` shape is `SELECT "<col>", count(*) ...`, so column 0 holds the values, in
/// the frequency order the query produced). NULLs are already filtered by the query; any that slip
/// through render as the empty string and are skipped (a NULL is not a completable value).
fn distinct_values(table: &crate::engine::Table) -> Vec<String> {
    let Some(col) = table.columns().first() else {
        return Vec::new();
    };
    col.cells
        .iter()
        .filter(|c| !c.is_null())
        .map(|c| c.display())
        .collect()
}

#[cfg(test)]
#[path = "app/app_tests.rs"]
mod app_tests;
