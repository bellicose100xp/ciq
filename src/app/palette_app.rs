//! Column-palette App orchestration — an `impl App` block lifted out of `app.rs` to keep that
//! file under the 1000-line cap. Owns opening / closing the SELECT-pane column picker, routing
//! its keys, and **mirroring every toggle into the SELECT pane immediately** (the live-rewrite
//! contract — see [`crate::palette`]).
//!
//! Headless: the palette state machine is pure data, and the only side effect is setting the
//! SELECT pane's text + scheduling a debounced query through the **same** dispatch path a typed
//! query uses (so there is no second engine entry).

use crate::app::query_form::SimplePane;
use crate::app::{App, AppPhase, Key, KeyEvent, QueryMode};

impl App {
    /// Open the column-picker popup (anchored to the SELECT pane).
    ///
    /// No-op:
    ///  * while loading / on a load error;
    ///  * with no schema (nothing to pick);
    ///  * in Power mode (the popup is a Simple-mode SELECT-pane affordance);
    ///  * when the focused pane is anything other than `SELECT`.
    ///
    /// On open the popup pre-checks against the live SELECT pane text (`*` or empty → all
    /// checked; comma list → only those checked). Closes the autocomplete popup so the two
    /// overlays never stack.
    pub(crate) fn open_palette(&mut self) {
        if matches!(self.phase, AppPhase::Loading | AppPhase::LoadError(_)) {
            return;
        }
        if self.palette.is_none() {
            return;
        }
        if !matches!(self.query_form.mode(), QueryMode::Simple) {
            return;
        }
        if self.query_form.focused_pane() != SimplePane::Select {
            return;
        }
        let select_text = self.query_form.text(SimplePane::Select).to_string();
        if let Some(palette) = self.palette.as_mut() {
            palette.open_with_select(&select_text);
        }
        self.autocomplete.close();
        self.palette_open = true;
    }

    /// Close the popup. The SELECT-pane text the user shaped via toggles stays as it is — the
    /// popup is a live editor, not an accept/cancel dialog.
    fn close_palette(&mut self) {
        self.palette_open = false;
    }

    /// Handle a key while the popup is open. Returns whether the app should quit (only `Ctrl+C`
    /// quits from here). Every toggle rewrites the SELECT pane and schedules a debounced
    /// dispatch. Keys (the user-locked map):
    ///  - `↑`/`↓`: cursor (bounded; no wrap).
    ///  - `Space` / `Tab`: toggle the highlighted column.
    ///  - `Enter`: close the popup (we're done).
    ///  - `Esc`: close the popup.
    ///  - `Ctrl+A`: select all.
    ///  - `Ctrl+X`: deselect all.
    ///  - `Ctrl+I`: invert.
    ///  - `Ctrl+C`: quit.
    ///  - any other key: consumed (does NOT fall through to the editor while the popup is open).
    pub(crate) fn handle_palette_key(&mut self, ev: &KeyEvent, now_ms: u64) -> bool {
        // Ctrl+C universally quits.
        if ev.mods.ctrl && matches!(ev.key, Key::Char('c') | Key::Char('C')) {
            return true;
        }
        if ev.mods.ctrl {
            match ev.key {
                Key::Char('a') | Key::Char('A') => {
                    if let Some(p) = self.palette.as_mut() {
                        p.select_all();
                    }
                    self.write_palette_to_select_and_schedule(now_ms);
                    return false;
                }
                Key::Char('x') | Key::Char('X') => {
                    if let Some(p) = self.palette.as_mut() {
                        p.deselect_all();
                    }
                    self.write_palette_to_select_and_schedule(now_ms);
                    return false;
                }
                Key::Char('i') | Key::Char('I') => {
                    if let Some(p) = self.palette.as_mut() {
                        p.invert();
                    }
                    self.write_palette_to_select_and_schedule(now_ms);
                    return false;
                }
                _ => {
                    // Other Ctrl chords are consumed (don't leak through to the editor).
                    return false;
                }
            }
        }
        match &ev.key {
            Key::Esc | Key::Enter => self.close_palette(),
            Key::Up => {
                if let Some(p) = self.palette.as_mut() {
                    p.cursor_up();
                }
            }
            Key::Down => {
                if let Some(p) = self.palette.as_mut() {
                    p.cursor_down();
                }
            }
            Key::Char(' ') | Key::Tab => {
                if let Some(p) = self.palette.as_mut() {
                    p.toggle_at_cursor();
                }
                self.write_palette_to_select_and_schedule(now_ms);
            }
            // All other keys are consumed while the popup owns the input.
            _ => {}
        }
        false
    }

    /// Write the palette's current checked set into the SELECT pane and schedule a debounced
    /// dispatch. The single seam every toggle/Ctrl+A/Ctrl+X/Ctrl+I goes through, so the
    /// SELECT-pane semantics (and the composer's empty-fallback to `*`) live in one place.
    pub(crate) fn write_palette_to_select_and_schedule(&mut self, now_ms: u64) {
        let new_select = match self.palette.as_ref() {
            Some(p) => p.write_to_select(),
            None => return,
        };
        self.query_form.set_text(SimplePane::Select, new_select);
        self.refresh_autocomplete();
        self.schedule(now_ms);
    }
}
