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

use crate::ai::ai_state::AiState;
use crate::autocomplete::autocomplete_state::AutocompleteState;
use crate::autocomplete::insertion::insert_suggestion;
use crate::autocomplete::value_source::ValueCache;
use crate::engine::InterruptHandle;
use crate::facets::FacetState;
use crate::facets::facet_query::build_facet_sql;
use crate::history::{HistoryState, storage as history_storage};
use crate::palette::PaletteState;
use crate::palette::query_emit::emit_with_limit as emit_palette_with_limit;
use crate::query::debouncer::Debouncer;
use crate::query::dispatcher::Dispatcher;
use crate::query::error_enhance::{enhance, enhance_with_schema};
use crate::query::preprocess::{PreprocessError, applies_viewport_limit, prepare_interactive};
use crate::query::worker::types::{ProcessedResult, QueryRequest, QueryResponse, RequestKind};
use crate::schema::Schema;

use std::path::PathBuf;
use std::sync::mpsc::Sender;

pub mod app_render;
pub mod autocomplete_app;
pub mod editor;
pub mod event_loop;
pub mod key;
pub mod polish;

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

/// What handling a key while the facet popup is open resolves to (P4.6): quit the app, consume the
/// key (Esc closed the popup), or close the popup and fall through to normal routing.
enum FacetKey {
    /// Ctrl-C: quit the app.
    Quit,
    /// Esc: the popup closed; the key is fully handled.
    Consumed,
    /// Any other key: the popup closed, but the key should still route normally.
    FallThrough,
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
///
/// A handful of fields/methods are `pub(crate)` so the cohesive `impl App` blocks lifted into
/// sibling files to respect the 1000-line cap — the AI orchestration ([`crate::ai::ai_app`]), the
/// history orchestration ([`crate::history::history_app`]), and the autocomplete orchestration
/// ([`autocomplete_app`]) — can drive their popups and reuse the same schedule/refresh path a typed
/// query uses.
pub struct App {
    pub(crate) phase: AppPhase,
    focus: Focus,
    pub(crate) editor: Editor,
    debouncer: Debouncer,
    dispatcher: Dispatcher,
    /// The latest accepted successful result, if any (None until the first query lands).
    result: Option<ProcessedResult>,
    /// Whether the most recently *dispatched* query had ciq's viewport `LIMIT` wrap applied (the
    /// user supplied no `LIMIT` of their own). When the landed result's row count reaches that cap,
    /// the grid is truncated and a banner is shown (P5.3). Recomputed on each dispatch; carried to
    /// the response so a stale value never mislabels a result.
    last_query_ciq_capped: bool,
    /// Whether the *displayed* result was ciq-capped (the accepted response's query was wrapped).
    /// Drives the truncation banner together with the result's row count.
    result_ciq_capped: bool,
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
    pub(crate) schema: Option<Schema>,
    /// The autocomplete popup state machine (P3.6) — recomputed on each query-bar edit.
    pub(crate) autocomplete: AutocompleteState,
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
    pub(crate) palette_open: bool,
    /// The instant-facet popup (P4.6, §6.5): the focused column + its parsed stats, filled
    /// out-of-band by the worker. `None` when no facet is open. Opened by `f` on the focused grid
    /// column; the SQL rides the same worker channel and the response routes here (not the grid) by
    /// [`RequestKind::Facet`].
    facet: Option<FacetState>,
    /// The query-history ring (P5.2, §7.6): prior SQL queries, newest first. Recorded on each
    /// successful dispatch; recalled through the popup. In-memory always; persisted to disk when
    /// [`history_path`](Self::history_path) is set + [`history_persist`](Self::history_persist).
    pub(crate) history: HistoryState,
    /// Whether the history popup is currently open (intercepts keys like the palette popup).
    pub(crate) history_open: bool,
    /// The on-disk history file path (from `[history] path`, or the XDG default). `None` = no
    /// persistence wired (the in-memory ring still works). Tests always pass a tempdir path here,
    /// never `$HOME`.
    pub(crate) history_path: Option<PathBuf>,
    /// The history entry cap (from `[history] max_entries`); bounds the on-disk file.
    pub(crate) history_max: usize,
    /// Whether on-disk history persistence is enabled (`[history] enabled`). When false, the ring
    /// is session-only.
    pub(crate) history_persist: bool,
    /// The AI NL->SQL popup state (P5.1): the natural-language prompt the user types + the request
    /// lifecycle (editing / pending / success / error). Pure state; the request itself runs on the
    /// AI thread (see [`ai_app`](crate::ai::ai_app)).
    pub(crate) ai: AiState,
    /// The request channel to the AI thread — `Some` only when the `[ai]` feature is active and the
    /// thread was spawned. The App sends a built prompt; the thread calls `Provider::complete` and
    /// sends the result back on a channel the event loop drains. `None` keeps the AI popup
    /// unavailable (the chord is a no-op).
    pub(crate) ai_tx: Option<Sender<crate::ai::ai_app::AiJob>>,
    /// Monotonic AI-request sequence id (P5.1): bumped on each submit so a reply from a superseded
    /// request is discarded (stale-discard, like the query worker's `request_id`).
    pub(crate) ai_seq: u64,
    /// The interactive viewport row cap (`[general] row_limit`): the `LIMIT N` a bare `SELECT` is
    /// wrapped to, and the cap the truncation banner compares against. Defaults to
    /// [`VIEWPORT_ROW_LIMIT`]; the event loop overrides it from the config via
    /// [`configure_general`](Self::configure_general).
    viewport_row_limit: usize,
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
            last_query_ciq_capped: false,
            result_ciq_capped: false,
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
            facet: None,
            history: HistoryState::new(),
            history_open: false,
            history_path: None,
            history_max: crate::config::history_config::DEFAULT_MAX_ENTRIES,
            history_persist: false,
            ai: AiState::new(),
            ai_tx: None,
            ai_seq: 0,
            viewport_row_limit: VIEWPORT_ROW_LIMIT,
        }
    }

    // --- read-only accessors (tests assert on structured state, not just the screen) ---

    pub fn phase(&self) -> &AppPhase {
        &self.phase
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// The full query text (the textarea lines joined with `\n`). Owned because the multiline
    /// buffer joins on demand; equality assertions against a `&str` still work.
    pub fn query(&self) -> String {
        self.editor.text()
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    /// The query bar's current vim mode (`INSERT`/`NORMAL`/…), surfaced in the status line and
    /// (later) the help bar so the mode is always visible.
    pub fn editor_mode(&self) -> crate::app::editor::EditorMode {
        self.editor.mode()
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

    /// The open facet popup state, if any (for the render layer and tests).
    pub fn facet(&self) -> Option<&FacetState> {
        self.facet.as_ref()
    }

    /// Whether the facet popup is currently open.
    pub fn is_facet_open(&self) -> bool {
        self.facet.is_some()
    }

    /// The query-history ring (for the render layer and tests).
    pub fn history(&self) -> &HistoryState {
        &self.history
    }

    /// Whether the history popup is currently open.
    pub fn is_history_open(&self) -> bool {
        self.history_open
    }

    /// The AI NL->SQL popup state (for the render layer and tests).
    pub fn ai(&self) -> &crate::ai::AiState {
        &self.ai
    }

    /// Whether the AI popup is currently open.
    pub fn is_ai_open(&self) -> bool {
        self.ai.is_open()
    }

    /// Configure history persistence (P5.2): the on-disk file path, the entry cap, and whether to
    /// persist at all (from the `[history]` config section). Seeds the in-memory ring from the file
    /// when persistence is enabled and a path is set. The event loop calls this once at startup
    /// with the resolved config; tests pass a tempdir path so the suite never touches `$HOME`.
    pub fn configure_history(&mut self, path: Option<PathBuf>, max_entries: usize, persist: bool) {
        self.history_max = max_entries.max(1);
        self.history_persist = persist;
        self.history_path = path;
        if self.history_persist
            && let Some(p) = self.history_path.as_ref()
        {
            // Seed the in-session ring from disk, capped to the same `max_entries` the on-disk file
            // is bounded by — so the documented "cap bounds the in-session ring and the file" holds.
            self.history =
                HistoryState::with_entries_max(history_storage::load(p), self.history_max);
        } else {
            // Session-only ring: still bound it to the configured cap.
            self.history.set_max(self.history_max);
        }
    }

    /// Configure the interactive viewport row cap (`[general] row_limit`). The event loop calls
    /// this once at startup with the resolved config so a bare `SELECT` is wrapped to the user's
    /// configured `LIMIT N` (and the truncation banner compares against the same cap). The config
    /// accessor already clamps `0` to `1`; this clamps defensively too.
    pub fn configure_general(&mut self, row_limit: usize) {
        self.viewport_row_limit = row_limit.max(1);
    }

    /// The effective interactive viewport row cap (`[general] row_limit`, defaulted).
    pub fn viewport_row_limit(&self) -> usize {
        self.viewport_row_limit
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

    /// The large-result truncation banner (P5.3), or `None` when the grid isn't ciq-capped. Shown
    /// when ciq wrapped the query in its viewport `LIMIT` and the displayed row count reached the
    /// cap — derived from state, no extra COUNT query (see [`polish::truncation_banner`]).
    pub fn truncation_banner(&self) -> Option<String> {
        let rows = self.result.as_ref()?.rows.row_count();
        polish::truncation_banner(rows, self.viewport_row_limit, self.result_ciq_capped)
    }

    /// The empty-state message for the results pane when there is no grid to draw (P5.3):
    /// `Loading` while parsing, `ZeroRows` when a query matched nothing, `NoQueryYet` before the
    /// first query. `None` when a populated result exists (the grid draws instead).
    pub fn empty_state(&self) -> Option<&'static str> {
        if matches!(self.phase, AppPhase::Loading) {
            return Some(polish::empty_state(polish::EmptyKind::Loading));
        }
        match self.result.as_ref() {
            Some(result) if result.rows.row_count() == 0 => {
                Some(polish::empty_state(polish::EmptyKind::ZeroRows))
            }
            Some(_) => None, // a populated grid is drawn, not an empty-state message
            // No result yet: the initial "type a query" hint. (A query that errored leaves the
            // error in the status line and no grid; the neutral hint is the right pane content.)
            None => Some(polish::empty_state(polish::EmptyKind::NoQueryYet)),
        }
    }

    // --- event routing (headless: synthetic KeyEvents) ---

    /// Route one key event to the focused surface, scheduling a debounced query when the query
    /// text changes. `now_ms` is the synthetic/real time stamp for the debouncer (never a wall
    /// clock read inside this fn). Returns `true` if the app should quit.
    ///
    /// When the column palette is open it intercepts ALL keys (toggle / reorder / filter / emit /
    /// close) *first* — so Esc closes the palette and Ctrl-C still quits, but typing filters
    /// columns rather than editing the bar. When the autocomplete popup is open it then intercepts
    /// Tab/Enter (accept), Up/Down (move selection), and Esc (dismiss) before the focus routing —
    /// so Esc dismisses the popup rather than quitting (it only quits when no popup is open).
    /// `Ctrl+K` opens the palette from anywhere.
    pub fn on_key(&mut self, ev: KeyEvent, now_ms: u64) -> bool {
        if self.ai.is_open() {
            return self.handle_ai_key(&ev, now_ms);
        }
        if self.history_open {
            return self.handle_history_key(&ev, now_ms);
        }
        if self.palette_open {
            return self.handle_palette_key(&ev, now_ms);
        }
        // The facet popup, when open, intercepts Esc (close) and Ctrl-C (quit) before the rest of
        // routing — so Esc dismisses the popup rather than quitting. Any other key closes it and
        // falls through to normal routing (e.g. arrows resume grid scrolling).
        if self.facet.is_some() {
            match self.handle_facet_key(&ev) {
                FacetKey::Quit => return true,
                FacetKey::Consumed => return false,
                FacetKey::FallThrough => {} // popup closed; route the key normally below
            }
        }
        // Ctrl+K opens the column palette (when a schema is loaded). Checked before the popup /
        // quit routing so the chord is reachable from any non-palette state.
        if ev.mods.ctrl && matches!(ev.key, Key::Char('k') | Key::Char('K')) {
            self.open_palette();
            return false;
        }
        // Ctrl+R opens the query-history popup (the recall chord, §7.6). Reachable from any
        // non-popup state, like Ctrl+K. Seeds the needle with the current bar text so the list
        // pre-filters to similar prior queries.
        if ev.mods.ctrl && matches!(ev.key, Key::Char('r') | Key::Char('R')) {
            self.open_history();
            return false;
        }
        // Ctrl+G opens the AI NL->SQL popup (the "generate" chord, P5.1). No-op when the AI
        // feature is inactive (no provider configured) or while loading. Reachable from any
        // non-popup state, like Ctrl+K / Ctrl+R.
        if ev.mods.ctrl && matches!(ev.key, Key::Char('g') | Key::Char('G')) {
            self.open_ai();
            return false;
        }
        if self.autocomplete.is_open() && self.handle_popup_key(&ev, now_ms) {
            return false; // the popup consumed the key
        }
        // `f` in the results pane opens a facet for the focused (leftmost visible) grid column.
        if self.focus == Focus::Results && matches!(ev.key, Key::Char('f') | Key::Char('F')) {
            self.open_facet();
            return false;
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

    /// Handle a key while the facet popup is open: `Esc` closes it (consumed, no quit), `Ctrl-C`
    /// quits, any other key dismisses the popup and falls through to normal routing (e.g. arrows
    /// resume grid scrolling).
    fn handle_facet_key(&mut self, ev: &KeyEvent) -> FacetKey {
        if ev.mods.ctrl && matches!(ev.key, Key::Char('c') | Key::Char('C')) {
            return FacetKey::Quit;
        }
        match ev.key {
            Key::Esc => {
                self.close_facet();
                FacetKey::Consumed
            }
            _ => {
                self.close_facet();
                FacetKey::FallThrough
            }
        }
    }

    /// Open an instant facet for the focused grid column (P4.6, §6.5). The focused column is the
    /// leftmost visible one (`h_col_offset`) of the current result; it must resolve to a real base
    /// table column (the facet queries `t`), so a derived/expression column with no schema match is
    /// a no-op. Dispatches the type-aware aggregate SQL through the worker (same channel/engine) and
    /// shows a pending popup until the response lands. No-op without a result or a schema.
    fn open_facet(&mut self) {
        let Some(schema) = self.schema.as_ref() else {
            return;
        };
        let Some(result) = self.result.as_ref() else {
            return;
        };
        let Some(column) = result.rows.columns().get(self.h_col_offset) else {
            return;
        };
        // Resolve the result column name to the base-table schema (the facet queries `t`); skip a
        // derived column that isn't a real table column.
        let Some(meta) = schema.column_ci(&column.name) else {
            return;
        };
        let name = meta.name.clone();
        let ty = meta.ty.clone();
        let sql = build_facet_sql(&name, schema);
        if self.dispatcher.dispatch_facet(sql, name.clone()).is_ok() {
            self.facet = Some(FacetState::pending(name, ty));
        }
    }

    /// Close the facet popup.
    fn close_facet(&mut self) {
        self.facet = None;
    }

    /// Open the column palette (P4.5/D3). No-op while loading / on a load error, or with no schema
    /// (nothing to pick). Closes the autocomplete popup so the two overlays never stack. The
    /// palette keeps whatever selection/needle state it already had (so reopening resumes where the
    /// user left off).
    fn open_palette(&mut self) {
        if self.palette.is_none()
            || matches!(self.phase, AppPhase::Loading | AppPhase::LoadError(_))
        {
            return;
        }
        self.autocomplete.close();
        self.palette_open = true;
    }

    /// Close the palette popup without emitting.
    fn close_palette(&mut self) {
        self.palette_open = false;
    }

    /// Handle a key while the palette is open (P4.5). Returns whether the app should quit (only
    /// `Ctrl+C` quits from here; `Esc` closes the palette). The keys, mirroring the §6.2 chord set:
    ///  - `Space` toggles the column under the cursor (checked <-> unchecked);
    ///  - `Up`/`Down` move the cursor through the filtered list;
    ///  - `Left`/`Right` reorder the cursor's checked column earlier/later in the projection;
    ///  - a printable char appends to the fuzzy needle; `Backspace` pops it;
    ///  - `Enter` emits the palette's query into the bar (-> debouncer -> worker) and closes;
    ///  - `Esc` closes without emitting.
    fn handle_palette_key(&mut self, ev: &KeyEvent, now_ms: u64) -> bool {
        if ev.mods.ctrl && matches!(ev.key, Key::Char('c') | Key::Char('C')) {
            return true; // Ctrl-C still quits
        }
        let Some(palette) = self.palette.as_mut() else {
            self.palette_open = false;
            return false;
        };
        match &ev.key {
            Key::Esc => self.close_palette(),
            Key::Enter => self.emit_palette_query(now_ms),
            Key::Char(' ') => palette.toggle_cursor(),
            Key::Up => palette.cursor_up(),
            Key::Down => palette.cursor_down(),
            Key::Left => {
                if let Some(i) = palette.cursor_column_index() {
                    palette.move_selection_up(i);
                }
            }
            Key::Right => {
                if let Some(i) = palette.cursor_column_index() {
                    palette.move_selection_down(i);
                }
            }
            Key::Char(c) => palette.push_needle(*c),
            Key::Backspace => palette.pop_needle(),
            _ => {}
        }
        false
    }

    /// Emit the palette's generated query into the bar, record it as palette-owned, schedule it
    /// (-> debouncer -> worker, the normal path), and close the palette. The single point where a
    /// palette `Enter` reaches the engine — through exactly the same dispatch path a typed query
    /// uses, so there is no second engine entry.
    fn emit_palette_query(&mut self, now_ms: u64) {
        let limit = self.viewport_row_limit;
        let Some(palette) = self.palette.as_mut() else {
            self.palette_open = false;
            return;
        };
        let sql = emit_palette_with_limit(palette, limit);
        palette.record_emitted(&sql);
        self.editor.set_text(&sql);
        self.refresh_autocomplete();
        self.close_palette();
        self.schedule(now_ms);
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
            insert_suggestion(&self.editor.text(), self.editor.cursor_byte(), &suggestion);
        self.editor.set_text_with_byte_cursor(new_text, new_cursor);
        self.autocomplete.close();
        // The inserted text changed the query — schedule the debounced grid query for it.
        self.schedule(now_ms);
    }

    fn on_key_query_bar(&mut self, ev: KeyEvent, now_ms: u64) {
        if matches!(self.phase, AppPhase::LoadError(_)) {
            return; // bar is frozen once load failed
        }
        // Vim modal routing: in any non-Insert mode, the key is a vim command (motion / edit /
        // mode flip), not text. `Esc` in Insert mode drops to Normal (the one Insert key vim owns).
        // Everything else in Insert mode is the text-editing path below (typing, Enter=newline,
        // autocomplete) — unchanged, so the live-query + completion wiring is untouched.
        if !self.editor.mode().is_insert() || matches!(ev.key, Key::Esc) {
            let changed = self.editor.on_vim_key(&ev);
            // A vim edit changed the cursor context (and possibly the text) — recompute the popup.
            self.refresh_autocomplete();
            if changed {
                self.schedule(now_ms);
            }
            return;
        }
        let before = self.editor.text();
        match ev.key {
            Key::Char(c) => self.editor.insert_char(c),
            Key::Backspace => {
                self.editor.backspace();
            }
            Key::Delete => {
                self.editor.delete();
            }
            Key::Left => self.editor.move_left(),
            Key::Right => self.editor.move_right(),
            Key::Home => self.editor.move_home(),
            Key::End => self.editor.move_end(),
            // Within a multiline query, Up moves between lines; on the first line it is a no-op
            // (there is nowhere above the bar to go).
            Key::Up => self.editor.move_up(),
            // Enter (and Shift+Enter) insert a newline — newline universally, since queries run
            // live on debounce and there is no submit key (locked decision).
            Key::Enter => self.editor.insert_newline(),
            Key::Paste(ref s) => self.editor.insert_str(s),
            // Down moves between lines in a multiline query; from the last line it hands navigation
            // off to the results grid (the single-line case, so the felt behavior is unchanged).
            Key::Down => {
                if self.editor.is_on_last_line() {
                    self.focus = Focus::Results;
                } else {
                    self.editor.move_down();
                }
            }
            _ => {}
        }
        // Recompute autocomplete on any edit/cursor move (the popup tracks the cursor context).
        self.refresh_autocomplete();
        // Only (re)schedule a query when the text actually changed — pure cursor moves don't.
        if self.editor.text() != before {
            self.schedule(now_ms);
        }
    }

    fn on_key_results(&mut self, ev: KeyEvent) {
        match ev.key {
            Key::Up if self.v_row_offset == 0 => {
                // Returning focus to the bar lands in Insert mode so typing resumes immediately.
                self.focus = Focus::QueryBar;
                self.editor.reset_to_insert();
            }
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
    pub(crate) fn schedule(&mut self, now_ms: u64) {
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
        let raw = self.editor.text().trim().to_string();
        if raw.is_empty() {
            // Clearing the bar returns to the pre-first-query empty state (the "type a query" hint),
            // not a zero-row *result* — drop the result so the empty-state picks the right line.
            self.clear_result();
            self.status = "ready".to_string();
            self.phase = AppPhase::Ready;
            return false;
        }
        match prepare_interactive(&raw, self.viewport_row_limit) {
            Ok(sql) => match self.dispatcher.dispatch(sql) {
                Ok(_id) => {
                    // Remember whether ciq wrapped this query in its viewport LIMIT (the user
                    // supplied none), so a result hitting the cap can show a truncation banner
                    // (P5.3) — derived from the raw query, no extra COUNT query.
                    self.last_query_ciq_capped = applies_viewport_limit(&raw);
                    // Record the raw user query (not the LIMIT-wrapped SQL) in history — this is
                    // the felt "I ran this" moment, and only a query that passed the read-only
                    // single-statement guard reaches here (§7.6).
                    self.record_history(&raw);
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
        // Stay Ready (the bar is still live) but show no stale grid for an invalid query — drop the
        // last-good result so the pane falls to the neutral empty-state, matching this fn's contract
        // and the `empty_state` doc ("a query that errored leaves no grid").
        self.clear_result();
        self.phase = AppPhase::Ready;
    }

    /// Drop the displayed result and reset its scroll/cap bookkeeping so the results pane falls back
    /// to the neutral empty-state. Used on both error paths (preprocess reject + engine `Error`) and
    /// when the bar is cleared, so a stale grid never lingers under an error message.
    fn clear_result(&mut self) {
        self.result = None;
        self.result_ciq_capped = false;
        self.v_row_offset = 0;
        self.h_col_offset = 0;
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
            let values = autocomplete_app::distinct_values(&result.rows);
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
        // Facet fetches are also out of the main lane (P4.6): route a success to the open facet
        // popup by its column (ignore one for a *different* column — a stale facet the user has since
        // closed/replaced), never through the stale-discard gate. An error/cancel leaves the popup
        // pending (it just shows "computing…"); the user can Esc out.
        if let QueryResponse::ProcessedSuccess {
            result,
            kind: RequestKind::Facet { column },
            ..
        } = &resp
        {
            if let Some(facet) = self.facet.as_mut()
                && facet.column() == column
            {
                facet.apply_result(&result.rows);
            }
            return false; // the facet popup filled; the grid is unchanged
        }
        if let QueryResponse::Error {
            kind: RequestKind::Facet { .. },
            ..
        }
        | QueryResponse::Cancelled {
            kind: RequestKind::Facet { .. },
            ..
        } = &resp
        {
            return false; // a failed/superseded facet leaves the popup pending; Esc dismisses it
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
                self.result_ciq_capped = self.last_query_ciq_capped;
                self.v_row_offset = 0;
                self.phase = AppPhase::Ready;
                true
            }
            QueryResponse::Error { message, .. } => {
                // Enhance against the loaded schema when present — an unknown column gets a local
                // "did you mean?" suggestion (P5.3); no schema falls back to the plain mapping.
                self.status = match self.schema.as_ref() {
                    Some(schema) => enhance_with_schema(&message, schema),
                    None => enhance(&message),
                };
                // Drop the last-good grid so an error never leaves a stale, mislabeled result (and
                // its truncation banner) painted under the error message (empty_state contract).
                self.clear_result();
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
            .is_some_and(|p| p.owns(&self.editor.text()))
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
        let limit = self.viewport_row_limit;
        let Some(palette) = self.palette.as_mut() else {
            return false;
        };
        let sql = emit_palette_with_limit(palette, limit);
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
        let limit = self.viewport_row_limit;
        let palette = self.palette.as_mut()?;
        let sql = emit_palette_with_limit(palette, limit);
        palette.record_emitted(&sql);
        self.editor.set_text(&sql);
        self.refresh_autocomplete();
        self.schedule(now_ms);
        Some(sql)
    }

    // --- query history (P5.2, §7.6): the open/close/handle/recall/record block lives in
    //     `crate::history::history_app` (an `impl App` block) to keep this file under the
    //     1000-line cap; `configure_history` + the accessors stay here with the other accessors. ---

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
