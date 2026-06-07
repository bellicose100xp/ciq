// ciq:shell-exempt
//! The crossterm terminal edge — the ONE place ciq touches a real terminal and the wall clock
//! at the app layer (`dev/PLAN.md` §3.1, §4.7 rows 1+2+3+5).
//!
//! Everything testable has been pulled out of here into the headless core: event routing
//! ([`App::on_key`](super::App::on_key)), the debounce decision
//! ([`App::tick`](super::App::tick)), response handling ([`App::on_response`]), and the entire
//! render path ([`app_render`](super::app_render)). What remains is irreducible plumbing that a
//! `TestBackend` cannot exercise and an agent cannot self-validate:
//!
//! - raw-mode enter/leave + alternate screen + mouse + bracketed-paste enable (§4.7 row 2/3),
//! - the real `crossterm` event poll and the decode of a `crossterm::event::Event` into the
//!   neutral [`KeyEvent`](super::KeyEvent) the core understands (§4.7 row 2),
//! - reading the wall clock to feed the debouncer real time (the determinism seam — see the
//!   `now_ms` reads below; this file is the app-layer analog of `logging.rs`),
//! - flushing styled cells to a real `CrosstermBackend` (§4.7 row 1), and the live resize
//!   (§4.7 row 5).
//!
//! It is **shell-exempt** (the marker above; listed in `dev/shell-exempt.txt`) precisely because
//! it is the §4.7 human surface — it is exercised by the scripted human smoke, not the headless
//! suite.
//!
//! ## Wall-clock seam
//!
//! This file reads `Instant::now()` to feed the debouncer real elapsed time (`now_ms`). That is
//! the only ambient clock at the app layer and is confined here behind
//! `#[allow(clippy::disallowed_methods)]` — the same documented seam pattern as `logging.rs` and
//! the debouncer's `system_time_ms()`. The *logic* the clock feeds (`App::tick`,
//! `Debouncer::should_execute_at`) takes the time as a `u64` parameter and is deterministic; the
//! read itself can only live at this irreducible terminal edge.

use std::io::{Stdout, stdout};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread;
use std::time::Instant;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEventKind, KeyModifiers,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use crate::engine::{CsvOpts, DuckdbEngine, InterruptHandle, QueryEngine};
use crate::query::worker::spawn_worker;
use crate::query::worker::types::{QueryRequest, QueryResponse};
use crate::schema::Schema;

use super::{App, Key, KeyEvent, KeyMods};

/// How long to block on a single crossterm event poll before looping to drive the debouncer /
/// drain responses. Short enough that a debounce fire is never delayed perceptibly.
const POLL_MS: u64 = 16;

/// What the off-thread loader hands back once the CSV ingest finishes.
enum LoadOutcome {
    /// Loaded; carries the engine's interrupt handle, the loaded schema (the autocomplete
    /// candidate source), a one-line summary for the status line, and the effective CSV dialect
    /// (delimiter, header) for the schema bar (`None` delimiter = DuckDB auto-detected it).
    Ready {
        interrupt: InterruptHandle,
        schema: Schema,
        summary: String,
        delimiter: Option<char>,
        header: bool,
    },
    /// Ingest failed; carries the error message for the `LoadError` state.
    Failed(String),
}

/// Run the full interactive session against the CSV at `path`. Sets up the terminal, spawns the
/// off-thread loader+worker, drives the event loop until quit, and restores the terminal on the
/// way out (even on error). Blocking; returns when the user quits.
pub fn run(path: PathBuf, opts: CsvOpts) -> std::io::Result<()> {
    let mut terminal = setup_terminal()?;

    // Request channel created up front so the App exists (in Loading) before the engine does.
    let (request_tx, request_rx) = channel::<QueryRequest>();
    let (response_tx, response_rx) = channel::<QueryResponse>();
    let (load_tx, load_rx) = channel::<LoadOutcome>();

    // The launch dialect for the schema-bar summary (delimiter `None` = DuckDB auto-detected it).
    // Captured before `opts` moves into the loader thread.
    let summary_delim = opts.delimiter;
    let summary_header = opts.header.unwrap_or(true);

    // Loader+worker thread: load the CSV once (off the UI thread so the bar stays responsive),
    // report the outcome, then become the worker loop owning the engine.
    thread::spawn(move || {
        match DuckdbEngine::open(&path, &opts) {
            Ok(engine) => {
                let schema = engine.schema().clone();
                let rows = schema.len();
                let summary = format!("loaded: table t, {rows} column{}", plural(rows));
                let _ = load_tx.send(LoadOutcome::Ready {
                    interrupt: engine.interrupt_handle(),
                    schema,
                    summary,
                    delimiter: summary_delim,
                    header: summary_header,
                });
                // Hand the engine to the worker loop (it owns it for the session).
                let worker = spawn_worker(Box::new(engine), request_rx, response_tx);
                let _ = worker.join();
            }
            Err(e) => {
                let _ = load_tx.send(LoadOutcome::Failed(e.to_string()));
            }
        }
    });

    let mut app = App::new(request_tx, InterruptHandle::noop());

    // Wire query history from the `[history]` config section (P5.2). Loads + seeds the on-disk
    // ring up front so prior queries are recallable immediately. The config read is the shell
    // edge's filesystem touch (like the engine load); the App-level history behavior is headless.
    let cfg = crate::config::load_config().config;
    let hist = cfg.history();
    let history_path = hist
        .path()
        .map(std::path::PathBuf::from)
        .or_else(crate::history::storage::default_history_path);
    app.configure_history(history_path, hist.max_entries(), hist.enabled());

    // Wire the AI NL->SQL feature (P5.1) when `[ai]` is active. A provider is built from the
    // config (the API key is read from the env var it names, never the file); the AI thread owns
    // it and blocks on `Provider::complete` off the UI thread. When inactive, no thread is spawned
    // and the `Ctrl+G` chord is a no-op. The result receiver is drained in the event loop below.
    let ai_bridge =
        crate::ai::provider::provider_from_config(cfg.ai()).map(crate::ai::ai_app::spawn_ai_thread);
    let ai_result_rx = if let Some(bridge) = &ai_bridge {
        app.set_ai_channel(bridge.request_tx.clone());
        Some(&bridge.result_rx)
    } else {
        None
    };

    // The session's time origin. The one ambient clock read at this layer (the documented seam,
    // see module docs); everything downstream takes elapsed `u64` ms and stays deterministic.
    #[allow(clippy::disallowed_methods)]
    let start = Instant::now();

    let result = event_loop(
        &mut terminal,
        &mut app,
        &load_rx,
        &response_rx,
        ai_result_rx,
        start,
    );
    restore_terminal(&mut terminal)?;
    result
}

/// The inner loop, split out so terminal restoration always runs.
fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    load_rx: &Receiver<LoadOutcome>,
    response_rx: &Receiver<QueryResponse>,
    ai_result_rx: Option<&Receiver<crate::ai::ai_app::AiResult>>,
    start: Instant,
) -> std::io::Result<()> {
    loop {
        terminal.draw(|f| app.render(f))?;

        // Drain a load outcome (at most one per session).
        match load_rx.try_recv() {
            Ok(LoadOutcome::Ready {
                interrupt,
                schema,
                summary,
                delimiter,
                header,
            }) => {
                app.set_interrupt(interrupt);
                app.set_schema(schema);
                app.set_csv_summary(delimiter, header);
                app.on_loaded(summary);
                // Pre-seed the bar with the palette's own `SELECT * FROM t LIMIT n` so the common
                // "just opened a file" path starts palette-owned (§0/D3). No-op if the user typed
                // during load. `now_ms` is the documented wall-clock seam (see module docs).
                let now_ms = start.elapsed().as_millis() as u64;
                app.seed_palette_query(now_ms);
            }
            Ok(LoadOutcome::Failed(msg)) => app.on_load_error(msg),
            Err(_) => {}
        }

        // Drain any worker responses (stale ones are discarded inside the App).
        loop {
            match response_rx.try_recv() {
                Ok(resp) => {
                    app.on_response(resp);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        // Drain any AI NL->SQL results (P5.1). A successful reply drops generated SQL into the bar
        // and runs it through the normal preprocess-validate + dispatch path (no bypass); an error
        // surfaces in the popup. `now_ms` is the documented wall-clock seam.
        if let Some(rx) = ai_result_rx {
            loop {
                match rx.try_recv() {
                    Ok(res) => {
                        let now_ms = start.elapsed().as_millis() as u64;
                        app.on_ai_result(res, now_ms);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
        }

        // Poll for a real key/resize event; translate and route. `now_ms` is the one wall-clock
        // read at this layer (the documented seam) — it feeds the debouncer real time.
        if event::poll(std::time::Duration::from_millis(POLL_MS))? {
            match event::read()? {
                Event::Key(ke) if ke.kind == KeyEventKind::Press => {
                    if let Some(ev) = translate_key(ke) {
                        let now_ms = start.elapsed().as_millis() as u64;
                        if app.on_key(ev, now_ms) {
                            return Ok(()); // quit
                        }
                    }
                }
                Event::Paste(data) => {
                    let now_ms = start.elapsed().as_millis() as u64;
                    app.on_key(KeyEvent::plain(Key::Paste(data)), now_ms);
                }
                Event::Resize(_, _) => { /* next draw reflows from retained rows */ }
                _ => {}
            }
        }

        // Drive the debouncer once per turn with the current time.
        let now_ms = start.elapsed().as_millis() as u64;
        app.tick(now_ms);
    }
}

/// Translate a crossterm key event into the neutral [`KeyEvent`] the core understands. Returns
/// `None` for keys ciq doesn't model (so the loop ignores them).
fn translate_key(ke: event::KeyEvent) -> Option<KeyEvent> {
    let mods = KeyMods {
        ctrl: ke.modifiers.contains(KeyModifiers::CONTROL),
        alt: ke.modifiers.contains(KeyModifiers::ALT),
        shift: ke.modifiers.contains(KeyModifiers::SHIFT),
    };
    let key = match ke.code {
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Esc => Key::Esc,
        _ => return None,
    };
    Some(KeyEvent::new(key, mods))
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn setup_terminal() -> std::io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(
        out,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    Terminal::new(CrosstermBackend::new(out))
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> std::io::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}
