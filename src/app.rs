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
use crate::autocomplete::value_source::ValueCache;
use crate::engine::InterruptHandle;
use crate::facets::FacetState;
use crate::facets::facet_query::build_facet_sql;
use crate::history::{HistoryState, storage as history_storage};
use crate::palette::PaletteState;
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
pub mod help_line;
pub mod key;
pub mod layout_regions;
pub mod mouse;
pub mod mouse_app;
pub mod palette_app;
pub mod polish;
pub mod query_form;
pub mod query_form_app;

pub use editor::Editor;
pub use key::{Key, KeyEvent, KeyMods};
pub use layout_regions::{LayoutRegions, MouseTarget, PopupKind};
pub use mouse::MouseEvent;
pub use query_form::{QueryForm, QueryMode, SimplePane};

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
    /// The Simple/Power query form — five labeled clause panes (`SELECT` / `WHERE` / `GROUP BY` /
    /// `ORDER BY` / `LIMIT`) in Simple mode, or one full-SQL textarea in Power mode (toggle with
    /// `Ctrl+Q`). Default is **Simple**, with the cursor parked on `WHERE` and the other panes
    /// pre-seeded so the launch composes `SELECT * FROM t LIMIT 1000` and the grid populates
    /// immediately. All input — typing, vim chords, autocomplete inserts — routes through
    /// [`Self::input_editor_mut`] to the active editor (the focused pane in Simple, the textarea in
    /// Power), so the rest of the App is mode-agnostic.
    pub(crate) query_form: QueryForm,
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
    /// Whether the displayed result is stale — kept on screen DIMMED after a query-pipeline error
    /// (preprocess-reject of an attempted dispatch, or an engine `QueryResponse::Error`) so the
    /// last-good grid stays visible while the error message rides the status line (jiq's
    /// error-keeps-last-result-dimmed behavior). Cleared when the next successful response lands.
    /// Pane-validation errors (e.g. a non-numeric LIMIT in Simple mode) do NOT flip this — they
    /// never reach the engine, so the prior dispatched result keeps its NORMAL polarity.
    result_is_stale: bool,
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
    /// The SELECT-pane column picker (`Ctrl+P` from the SELECT pane). Built from the schema on
    /// load ([`set_schema`](Self::set_schema)); `None` until then. Every toggle in the popup
    /// rewrites the SELECT pane immediately — the popup is the live editor for the SELECT
    /// projection, not a separate emission with its own ownership semantics (the prior generated-
    /// state design, §0/D3, was replaced by the user-locked redesign 2026-06-09).
    palette: Option<PaletteState>,
    /// Whether the column-picker popup is currently open. When open it intercepts keys before the
    /// query-bar routing, like every other popup.
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
    /// The on-screen [`Rect`](ratatui::layout::Rect) of each mouse-routable surface, recorded by the
    /// render layer every frame ([`app_render`]) and read by [`on_mouse`](Self::on_mouse) to resolve
    /// a click/scroll to the surface under the pointer. A [`std::cell::Cell`] so the `&self` render
    /// path can update it without a `&mut` borrow (it is plain `Copy` geometry, not logic).
    layout_regions: std::cell::Cell<LayoutRegions>,
}

impl App {
    /// Build an `App` over the dispatcher's request channel + interrupt handle. Starts in
    /// `Loading` (the engine is ingesting off-thread).
    pub fn new(request_tx: Sender<QueryRequest>, interrupt: InterruptHandle) -> Self {
        Self {
            phase: AppPhase::Loading,
            focus: Focus::QueryBar,
            query_form: QueryForm::new(),
            debouncer: Debouncer::new(),
            dispatcher: Dispatcher::new(request_tx, interrupt),
            result: None,
            last_query_ciq_capped: false,
            result_ciq_capped: false,
            result_is_stale: false,
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
            layout_regions: std::cell::Cell::new(LayoutRegions::default()),
        }
    }

    // --- read-only accessors (tests assert on structured state, not just the screen) ---

    pub fn phase(&self) -> &AppPhase {
        &self.phase
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// The on-screen regions recorded by the last render pass (for tests asserting the mouse
    /// coordinate mapping, and read by [`on_mouse`](Self::on_mouse)).
    pub fn layout_regions(&self) -> LayoutRegions {
        self.layout_regions.get()
    }

    /// Record the on-screen regions for this frame. The render layer ([`app_render`]) calls this
    /// once per draw with the laid-out [`Rect`](ratatui::layout::Rect)s so the next mouse event
    /// resolves against the geometry the user actually sees. Kept off the `&mut self` path (it is
    /// `Copy` geometry, not logic) so the pure render fn need not borrow mutably.
    pub(crate) fn set_layout_regions(&self, regions: LayoutRegions) {
        self.layout_regions.set(regions);
    }

    /// The query string the dispatcher will send to the engine.
    ///
    /// In **Simple** mode this is the composed canonical SQL (e.g. `SELECT * FROM t LIMIT 1000`)
    /// from the five panes. In **Power** mode it is the textarea's text verbatim (multiline
    /// joined by `\n`). On a [`ComposeError`] (a non-numeric LIMIT pane, etc.) the empty string is
    /// returned so dispatch falls back to the empty-input path; callers should consult
    /// [`QueryForm::limit_error`] to surface a status-line message.
    pub fn query(&self) -> String {
        match self.query_form.mode() {
            QueryMode::Simple => self
                .query_form
                .to_full_sql(self.viewport_row_limit)
                .unwrap_or_default(),
            QueryMode::Power => self.query_form.power().text(),
        }
    }

    /// The active input editor — the focused Simple pane in Simple mode, or the Power textarea in
    /// Power mode. The single source of truth for "where typing goes," used by the render layer,
    /// autocomplete inserts, mouse cursor positioning, and tests.
    pub fn editor(&self) -> &Editor {
        match self.query_form.mode() {
            QueryMode::Simple => self.query_form.focused_editor(),
            QueryMode::Power => self.query_form.power(),
        }
    }

    /// Mutable view of the active input editor (see [`Self::editor`]). Routes typing /
    /// vim chords / autocomplete inserts to the right surface without callers having to branch on
    /// mode.
    pub(crate) fn input_editor_mut(&mut self) -> &mut Editor {
        match self.query_form.mode() {
            QueryMode::Simple => self.query_form.focused_editor_mut(),
            QueryMode::Power => self.query_form.power_mut(),
        }
    }

    /// The query bar's current vim mode (`INSERT`/`NORMAL`/…), surfaced in the status line and the
    /// help bar so the mode is always visible. Reads the active editor.
    pub fn editor_mode(&self) -> crate::app::editor::EditorMode {
        self.editor().mode()
    }

    /// Read-only view of the Simple/Power query form (the 5 panes + the Power textarea + the mode
    /// flag). The render layer reads it to branch on mode and to draw the focused pane's editor;
    /// tests reach in to assert pane text + focus without going through the App.
    pub fn query_form(&self) -> &QueryForm {
        &self.query_form
    }

    /// Mutable view of the form. Test-only — production paths use the more granular
    /// `query_form.set_text(…)` / `query_form.focus(…)` etc. directly. Gated `#[cfg(test)]` so
    /// clippy's dead-code lint doesn't fire on a release build (this is `pub(crate)` and only
    /// reached from `#[cfg(test)]` modules).
    #[cfg(test)]
    pub(crate) fn query_form_mut(&mut self) -> &mut QueryForm {
        &mut self.query_form
    }

    /// Test-only seam: switch the form into Power mode with the given SQL preloaded, so legacy
    /// tests that assert `app.query() == "<verbatim>"` keep their semantics. Production code never
    /// calls this — the chord users see is `Ctrl+Q` (which routes through [`Self::toggle_query_mode`]).
    #[cfg(test)]
    pub(crate) fn force_power_mode_for_tests(&mut self, sql: &str) {
        self.query_form.enter_power_with_sql(sql);
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    /// Set the status-line text. `pub(crate)` so the orchestration helpers (palette_app /
    /// history_app / ai_app) can surface a one-liner on a mode flip / simplifier refusal /
    /// generated-SQL acceptance without exposing the raw field.
    pub(crate) fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
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
    /// accessor already clamps `0` to `1`; this clamps defensively too. Also re-seeds the Simple-
    /// mode LIMIT pane so the form's composed SQL uses the same cap the dispatcher does.
    pub fn configure_general(&mut self, row_limit: usize) {
        self.viewport_row_limit = row_limit.max(1);
        self.query_form
            .set_default_limit_seed(self.viewport_row_limit);
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
    /// `Ctrl+P` opens the palette from anywhere.
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
        // Ctrl+T toggles keyboard focus between the query bar and the results pane (top-level so
        // it works from either focus state). Pane focus + scroll offsets are preserved across the
        // round trip — only the active surface changes.
        if ev.mods.ctrl && matches!(ev.key, Key::Char('t') | Key::Char('T')) {
            self.focus = match self.focus {
                Focus::QueryBar => Focus::Results,
                Focus::Results => Focus::QueryBar,
            };
            return false;
        }
        // Ctrl+P (column picker) is anchored to the SELECT pane in Simple mode — it's NOT a
        // top-level chord. The dispatch lives inside `on_key_query_bar` so the gate (focused pane
        // == SELECT) can be checked there cleanly. (Ctrl+K is reserved for tmux's HJKL pane-nav.)
        // Ctrl+R opens the query-history popup (the recall chord, §7.6). Reachable from any
        // non-popup state. Seeds the needle with the current bar text so the list pre-filters to
        // similar prior queries.
        if ev.mods.ctrl && matches!(ev.key, Key::Char('r') | Key::Char('R')) {
            self.open_history();
            return false;
        }
        // Ctrl+A opens the AI NL->SQL popup (the "ask" chord, P5.1). No-op when the AI feature is
        // inactive (no provider configured) or while loading. (Ctrl+G is intentionally unbound,
        // reserved for a future binding.)
        if ev.mods.ctrl && matches!(ev.key, Key::Char('a') | Key::Char('A')) {
            self.open_ai();
            return false;
        }
        // Ctrl+Q toggles between Simple (5 panes) and Power (free-form SQL) query modes. Simple ->
        // Power composes the panes into the textarea so refinement preserves context; Power ->
        // Simple parses the textarea via the simplifier and distributes into the panes — refusing
        // (with a clear status message) when the SQL has features Simple can't represent
        // (JOIN/CTE/HAVING/subquery/multi-statement/etc.). Mode flip schedules a re-dispatch via the
        // debouncer so the grid reflects the (possibly different) composed SQL.
        if ev.mods.ctrl && matches!(ev.key, Key::Char('q') | Key::Char('Q')) {
            self.toggle_query_mode(now_ms);
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

    // The column-palette orchestration (open/close/handle/emit + the seed/replace emitters) lives in
    // `crate::app::palette_app` (an `impl App` block) to keep this file under the 1000-line cap, like
    // the autocomplete/history/AI blocks; the pure palette accessors stay below with the others.

    /// Handle a key while the popup is open. Returns `true` if the popup consumed it (so the
    /// caller stops routing). Tab/Enter accept the selection; Up/Down move it; Esc dismisses; any
    /// other key falls through (e.g. typing keeps editing and recomputes the popup). `Shift+Tab`
    /// is intentionally unbound — `Up` already drives `select_prev`, so adding a redundant chord
    /// would just be noise.
    fn handle_popup_key(&mut self, ev: &KeyEvent, now_ms: u64) -> bool {
        match ev.key {
            Key::Tab if !ev.mods.shift => {
                self.accept_suggestion(now_ms);
                true
            }
            Key::Enter => {
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

    // The popup-input plumbing (`accept_suggestion`, `suggestion_target_editor_mut`) and the
    // query-bar key dispatch (`on_key_query_bar`) both live in `crate::app::query_form_app` (an
    // `impl App` block) to keep this file under the 1000-line cap, like the autocomplete /
    // history / AI / palette blocks. The popup is mode-aware: Simple-mode accepts land in the
    // focused pane editor; Power-mode accepts land in the App's `editor`.

    /// Toggle Simple ↔ Power query modes. Simple → Power composes the panes into the textarea so
    /// the user can refine without losing context; Power → Simple parses the textarea via the
    /// simplifier and on success distributes into the five panes (else surfaces "can't simplify:
    /// <reason>" and stays in Power). The mode flip schedules a re-dispatch through the existing
    /// debouncer so the grid reflects the (now possibly different) composed SQL.
    fn toggle_query_mode(&mut self, now_ms: u64) {
        match self.query_form.toggle_mode(self.viewport_row_limit) {
            Ok(()) => {
                let into = match self.query_form.mode() {
                    QueryMode::Simple => "simple",
                    QueryMode::Power => "power",
                };
                self.status = format!("query mode: {into}");
                self.refresh_autocomplete();
                self.schedule(now_ms);
            }
            Err(e) => {
                // The simplifier refused (Power → Simple has features it can't represent).
                // toggle_mode left the form in Power per its documented contract.
                self.status = format!("can't simplify: {}", e.message());
            }
        }
    }

    fn on_key_results(&mut self, ev: KeyEvent) {
        match ev.key {
            Key::Up if self.v_row_offset == 0 => {
                // Returning focus to the bar lands in Insert mode so typing resumes immediately.
                self.focus = Focus::QueryBar;
                self.input_editor_mut().reset_to_insert();
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

    // The mouse-routing impl block (on_mouse / scroll / click / popup) lives in
    // `crate::app::mouse_app` (an `impl App` block) to keep this file under the 1000-line cap; it is
    // a cohesive, self-contained seam over `LayoutRegions` + `MouseEvent`.

    /// (Re)schedule a debounced query at `now_ms`. While `Loading` we only remember that a query
    /// is pending (it can't run until the engine is `Ready`); the debounce window still gates it.
    pub(crate) fn schedule(&mut self, now_ms: u64) {
        self.debouncer.schedule_execution_at(now_ms);
        if matches!(self.phase, AppPhase::Loading) {
            self.pending_query_on_ready = true;
        }
    }

    /// Schedule the **initial** post-load query so the grid populates on launch without the user
    /// typing anything. In Simple mode the default panes always compose a non-empty SQL
    /// (`SELECT * FROM t LIMIT 1000`); in Power mode an empty textarea is a no-op. The event
    /// loop calls this once after [`on_loaded`].
    pub fn schedule_initial_query(&mut self, now_ms: u64) {
        if self.query().trim().is_empty() {
            return;
        }
        self.schedule(now_ms);
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
    ///
    /// In **Simple** mode, the dispatched text is the composed canonical SQL from the five panes
    /// (the user-visible text is the panes themselves; the composer assembles the dispatch). A
    /// non-numeric LIMIT pane fails composition with [`ComposeError::InvalidLimit`]: we surface its
    /// message in the status line, skip dispatch, and *do not* dim the prior result (a local
    /// pane-validation issue isn't an engine error). The next valid edit clears the limit error and
    /// dispatches.
    fn dispatch_current(&mut self) -> bool {
        // Composer fallibility (Simple mode only). Power mode is the textarea verbatim; never fails.
        if matches!(self.query_form.mode(), QueryMode::Simple) {
            match self.query_form.to_full_sql(self.viewport_row_limit) {
                Ok(_) => self.query_form.set_limit_error(None),
                Err(e) => {
                    self.query_form
                        .set_limit_error(Some(e.message().to_string()));
                    self.status = e.message().to_string();
                    // Pane-validation error: do NOT touch result_is_stale (the prior good result
                    // stays normal, not dimmed) and do NOT dispatch.
                    return false;
                }
            }
        }
        let raw = self.query().trim().to_string();
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
        // Keep the last-good grid in place but mark it stale so the render layer dims it (jiq's
        // error-keeps-last-result-dimmed behavior). Only flip the stale bit when there is something
        // to dim — otherwise this is the first-query case and the empty-state hint is correct.
        if self.result.is_some() {
            self.result_is_stale = true;
        }
        self.phase = AppPhase::Ready;
    }

    /// Drop the displayed result and reset its scroll/cap bookkeeping so the results pane falls back
    /// to the neutral empty-state. Used when the bar is cleared (a deliberate return to the pre-first-
    /// query state). Errors do NOT clear the result anymore — they dim it via `result_is_stale`.
    fn clear_result(&mut self) {
        self.result = None;
        self.result_ciq_capped = false;
        self.result_is_stale = false;
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
                // A successful response replaces the grid and clears any prior stale-dim — the
                // render layer goes back to NORMAL polarity for the new rows.
                self.result_is_stale = false;
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
                // Keep the last-good grid in place but mark it stale so the render layer dims it
                // (jiq's error-keeps-last-result-dimmed behavior). The error rides the status line.
                if self.result.is_some() {
                    self.result_is_stale = true;
                }
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
