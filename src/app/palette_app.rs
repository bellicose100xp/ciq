//! Column-palette App orchestration (`dev/PLAN.md` §6.2, `dev/DECISIONS.md` D3) — an `impl App`
//! block lifted out of `app.rs` to keep that file under the 1000-line cap, like the autocomplete /
//! history / AI blocks. It owns opening/closing the palette popup, routing its keys (toggle /
//! reorder / filter / emit / close), and the two emitter paths that install the palette's generated
//! SQL into the bar (the load-time seed and the explicit Replace).
//!
//! All of it is headless: the palette state machine is pure data, and the only side effects are
//! setting the bar text + scheduling a debounced query through the **same** dispatch path a typed
//! query uses (so there is no second engine entry).

use crate::app::{App, AppPhase, Key, KeyEvent};
use crate::palette::query_emit::emit_with_limit as emit_palette_with_limit;

impl App {
    /// Open the column palette (P4.5/D3). No-op while loading / on a load error, or with no schema
    /// (nothing to pick). Closes the autocomplete popup so the two overlays never stack. The
    /// palette keeps whatever selection/needle state it already had (so reopening resumes where the
    /// user left off).
    pub(crate) fn open_palette(&mut self) {
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
    pub(crate) fn handle_palette_key(&mut self, ev: &KeyEvent, now_ms: u64) -> bool {
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
}
