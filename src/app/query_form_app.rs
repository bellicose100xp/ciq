//! Query-form App orchestration — an `impl App` block lifted out of `app.rs` to keep that file
//! under the 1000-line cap, like the autocomplete / history / AI / palette blocks. It owns the
//! pieces of `App` that orbit the [`QueryForm`](crate::app::QueryForm): the `result_is_stale`
//! accessor (used by both the render layer and Stage 4 row counter), the suggestion-target seam
//! (where an accepted autocomplete suggestion lands — Simple pane vs. Power editor), and the
//! `accept_suggestion` driver that ties them together with the debouncer's schedule path.
//!
//! All of it is headless: pane edits are plain in-memory mutations and the only side effect is
//! scheduling a debounced query through the **same** dispatch path a typed query uses.

use crate::app::App;
use crate::app::editor::Editor;
use crate::app::{AppPhase, Focus, Key, KeyEvent, QueryMode};
use crate::autocomplete::insertion::insert_suggestion;

impl App {
    /// Whether the displayed result is stale (kept on screen dimmed after a query-pipeline
    /// error). Read by the render layer to apply [`crate::theme::grid::stale_modifier`] to the
    /// grid header + body (and by the row counter, which honors the same dim). `false` when
    /// there is no result or the most recent successful response replaced it.
    pub fn result_is_stale(&self) -> bool {
        self.result_is_stale
    }

    /// Insert the selected suggestion into the query at the cursor and dismiss the popup. The
    /// popup stays closed after an explicit accept (it does not re-open on the just-completed
    /// token); the next edit recomputes it for the new context. Closes without inserting if there
    /// is nothing selected.
    ///
    /// Targets the focused surface — the Simple-mode focused pane editor when the form is in
    /// Simple mode, the Power editor (= the App's `editor`) otherwise — so the just-completed
    /// text always lands where the user is typing.
    pub(crate) fn accept_suggestion(&mut self, now_ms: u64) {
        let Some(suggestion) = self.autocomplete.selected_suggestion().cloned() else {
            self.autocomplete.close();
            return;
        };
        let target = self.suggestion_target_editor_mut();
        let (new_text, new_cursor) =
            insert_suggestion(&target.text(), target.cursor_byte(), &suggestion);
        target.set_text_with_byte_cursor(new_text, new_cursor);
        self.autocomplete.close();
        // The inserted text changed the query — schedule the debounced grid query for it.
        self.schedule(now_ms);
    }

    /// The editor where an accepted suggestion should land. In Simple mode that's the focused
    /// pane's editor; in Power mode that's the textarea. A single seam so the popup never inserts
    /// into the wrong surface — delegates to the App's [`input_editor_mut`](App::input_editor_mut).
    pub(crate) fn suggestion_target_editor_mut(&mut self) -> &mut Editor {
        self.input_editor_mut()
    }

    /// Dispatch a key event while the query bar is focused. The autocomplete-popup-open routing,
    /// the global Ctrl chords (Ctrl+T / Ctrl+P / Ctrl+R / Ctrl+A / Ctrl+Q), and the popup-open
    /// dispatch all run UPSTREAM in [`App::on_key`]; this method handles everything that's left:
    /// pane navigation, vim modal commands, and the in-pane editor mutations.
    ///
    /// Pane navigation (Simple mode):
    ///  - `Alt+J` / `Alt+Down` → focus next pane, BOUNDED at LIMIT (no wrap; no-op at boundary).
    ///  - `Alt+K` / `Alt+Up` → focus previous pane, BOUNDED at SELECT (no-op at boundary).
    ///  - Plain `Up` / `Down` in the bar are no-ops in Simple mode (Power keeps move_up/down).
    ///  - `Shift+Tab` is unbound here (popup closed); when the popup is OPEN it drives
    ///    autocomplete `select_prev`, handled upstream in `handle_popup_key`.
    ///
    /// Tab (popup closed) inserts a literal `\t` at the cursor; Tab while the popup is open is
    /// already handled upstream as accept-suggestion.
    pub(super) fn on_key_query_bar(&mut self, ev: KeyEvent, now_ms: u64) {
        if matches!(self.phase, AppPhase::LoadError(_)) {
            return; // bar is frozen once load failed
        }
        // Ctrl+P opens the column-picker popup, **anchored to the SELECT pane in Simple mode**.
        // Outside that context (Power mode, or any other Simple pane focused) it is a silent
        // no-op — the popup is a SELECT-pane affordance only. Checked before pane nav so it isn't
        // shadowed by the modifier match below.
        if ev.mods.ctrl
            && matches!(ev.key, Key::Char('p') | Key::Char('P'))
            && matches!(self.query_form.mode(), QueryMode::Simple)
            && self.query_form.focused_pane() == crate::app::SimplePane::Select
        {
            self.open_palette();
            return;
        }
        // Pane navigation in Simple mode — Alt+J / Alt+K / Alt+Down / Alt+Up. Bounded (no wrap):
        // Alt+J/Down stops at LIMIT; Alt+K/Up stops at SELECT. Alt-modified keys never carry
        // editor text, so this is intercepted before the editor's typing path. Power mode ignores
        // (its single textarea has no pane to cycle).
        if ev.mods.alt
            && matches!(self.query_form.mode(), QueryMode::Simple)
            && matches!(
                ev.key,
                Key::Char('j')
                    | Key::Char('J')
                    | Key::Char('k')
                    | Key::Char('K')
                    | Key::Down
                    | Key::Up
            )
        {
            match ev.key {
                Key::Char('j') | Key::Char('J') | Key::Down => self.query_form.focus_next(),
                Key::Char('k') | Key::Char('K') | Key::Up => self.query_form.focus_prev(),
                _ => unreachable!(),
            }
            self.refresh_autocomplete();
            return;
        }
        // Tab when the popup is closed: insert a literal tab character at the cursor in the
        // focused-pane editor. (When the popup IS open, `handle_popup_key` upstream already
        // handled Tab as accept and Shift+Tab as select-prev — they never reach here.)
        // Shift+Tab with the popup closed is intentionally unbound (falls through; the editor
        // ignores it).
        if matches!(ev.key, Key::Tab) && !ev.mods.shift {
            self.input_editor_mut().insert_char('\t');
            self.refresh_autocomplete();
            self.schedule(now_ms);
            return;
        }
        // Vim modal routing: in any non-Insert mode, the key is a vim command (motion / edit /
        // mode flip), not text. `Esc` in Insert mode drops to Normal (the one Insert key vim owns).
        // Everything else in Insert mode is the text-editing path below (typing, Enter=newline,
        // autocomplete) — unchanged, so the live-query + completion wiring is untouched.
        if !self.editor().mode().is_insert() || matches!(ev.key, Key::Esc) {
            let changed = self.input_editor_mut().on_vim_key(&ev);
            // A vim edit changed the cursor context (and possibly the text) — recompute the popup.
            self.refresh_autocomplete();
            if changed {
                self.schedule(now_ms);
            }
            return;
        }
        let before = self.editor().text();
        match ev.key {
            Key::Char(c) => self.input_editor_mut().insert_char(c),
            Key::Backspace => {
                self.input_editor_mut().backspace();
            }
            Key::Delete => {
                self.input_editor_mut().delete();
            }
            Key::Left => self.input_editor_mut().move_left(),
            Key::Right => self.input_editor_mut().move_right(),
            Key::Home => self.input_editor_mut().move_home(),
            Key::End => self.input_editor_mut().move_end(),
            // Up: in Power, walk a line up within the multiline textarea. In Simple, plain Up is
            // a no-op — pane navigation is exclusively `Alt+K`/`Alt+Up` (bounded). The user
            // reaches the results pane via `Ctrl+T`, click, or the `f` chord.
            Key::Up => match self.query_form.mode() {
                QueryMode::Simple => {} // no-op (Alt+K / Alt+Up own pane nav)
                QueryMode::Power => self.input_editor_mut().move_up(),
            },
            // Enter (and Shift+Enter) insert a newline in Power; in Simple, panes are single-line
            // so the textarea swallows it as a no-op (its single-line nature is enforced by
            // construction).
            Key::Enter => self.input_editor_mut().insert_newline(),
            Key::Paste(ref s) => self.input_editor_mut().insert_str(s),
            // Down: in Power, walk a line down (and from the last line hand off to results). In
            // Simple, plain Down is a no-op — pane navigation is exclusively `Alt+J`/`Alt+Down`
            // (bounded), and `Ctrl+T` toggles to results.
            Key::Down => match self.query_form.mode() {
                QueryMode::Simple => {} // no-op (Alt+J / Alt+Down own pane nav)
                QueryMode::Power => {
                    if self.editor().is_on_last_line() {
                        self.focus = Focus::Results;
                    } else {
                        self.input_editor_mut().move_down();
                    }
                }
            },
            _ => {}
        }
        // Recompute autocomplete on any edit/cursor move (the popup tracks the cursor context).
        self.refresh_autocomplete();
        // Only (re)schedule a query when the text actually changed — pure cursor moves don't.
        if self.editor().text() != before {
            self.schedule(now_ms);
        }
    }
}
