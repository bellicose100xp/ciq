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

use crate::engine::InterruptHandle;
use crate::query::debouncer::Debouncer;
use crate::query::dispatcher::Dispatcher;
use crate::query::error_enhance::enhance;
use crate::query::preprocess::{PreprocessError, prepare_interactive};
use crate::query::worker::types::{ProcessedResult, QueryRequest, QueryResponse};

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

    // --- event routing (headless: synthetic KeyEvents) ---

    /// Route one key event to the focused surface, scheduling a debounced query when the query
    /// text changes. `now_ms` is the synthetic/real time stamp for the debouncer (never a wall
    /// clock read inside this fn). Returns `true` if the app should quit.
    pub fn on_key(&mut self, ev: KeyEvent, now_ms: u64) -> bool {
        // Ctrl-C / Esc quit from anywhere.
        if ev.is_quit() {
            return true;
        }
        match self.focus {
            Focus::QueryBar => self.on_key_query_bar(ev, now_ms),
            Focus::Results => self.on_key_results(ev),
        }
        false
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
        // Only (re)schedule a query when the text actually changed — pure cursor moves don't.
        if self.editor.text() != before {
            self.schedule(now_ms);
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
        let body_len = self.result.as_ref().map(|r| r.grid.body.len()).unwrap_or(0);
        let max = body_len.saturating_sub(1);
        self.v_row_offset = (self.v_row_offset + by).min(max);
    }

    fn scroll_right(&mut self) {
        let col_count = self.result.as_ref().map(|r| r.schema.len()).unwrap_or(0);
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

    /// Apply a [`QueryResponse`] from the worker. Stale responses (an older `request_id`) are
    /// dropped before touching result state (§0/D4). A worker-level panic (`request_id == 0`) is
    /// applied immediately. Returns `true` if the visible state changed.
    pub fn on_response(&mut self, resp: QueryResponse) -> bool {
        let id = resp.request_id();
        // request_id 0 marks a worker-level panic with no specific query — apply immediately.
        if id != 0 && !self.dispatcher.accept(id) {
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

#[cfg(test)]
#[path = "app/app_tests.rs"]
mod app_tests;
