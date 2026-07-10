//! Save-popup + exit-output App orchestration (`Ctrl+W` / `Ctrl+O`) — an `impl App` block lifted
//! out of `app.rs` (like `search_app` / `palette_app`) to keep that file under the line cap.
//!
//! Two related deliverables, both over the **displayed** result (the Ctrl+F-filtered view when a
//! filter is active — output what you see):
//! - `Ctrl+O` sets the [`ExitAction::PrintResult`] flag and quits; the event loop prints the
//!   console-styled grid ([`crate::output::render_console`]) after the terminal is restored, so
//!   the result lands in the scrollback (jiq's output-on-exit).
//! - `Ctrl+W` opens the save popup: type a filename, Enter resolves it (tilde-expanded, `.csv`
//!   defaulted) and writes the RFC-4180 CSV ([`crate::output::render_output`]) — the same bytes
//!   the `--output csv` path emits.
//!
//! The filename editing and preview recompute are headless; the one I/O is the resolve-probe +
//! write through [`crate::save::save_io`], tempdir-tested like the history storage seam.

use std::path::PathBuf;

use crate::app::{App, ExitAction, Key, KeyEvent};
use crate::output::{OutputFormat, render_output};
use crate::save::save_io;
use crate::save::save_state::PathPreview;

impl App {
    /// The save popup state (for the render layer and tests).
    pub fn save(&self) -> &crate::save::SaveState {
        &self.save
    }

    /// Whether the save popup is currently open (it captures the keyboard while open).
    pub fn is_save_open(&self) -> bool {
        self.save.is_open()
    }

    /// The action the shell should take after quitting, if any (read by the event loop once the
    /// terminal is restored).
    pub fn exit_action(&self) -> Option<ExitAction> {
        self.exit_action
    }

    /// Install the save-popup context: the source CSV's stem (seeds the default filename
    /// `<stem>-out.csv`) and the home directory for `~` expansion. The event loop calls this once
    /// at startup; tests pass a tempdir so the suite never reads `$HOME`.
    pub fn configure_save(&mut self, source_stem: Option<String>, home: Option<PathBuf>) {
        self.source_stem = source_stem;
        self.save_home = home;
    }

    /// Quit with the displayed result printed to the scrollback (`Ctrl+O`). With no result on
    /// screen there is nothing to print — quit plain, like `Ctrl+C`. Returns `true` (quit) either
    /// way.
    pub(crate) fn quit_with_print(&mut self) -> bool {
        if self.display_rows().is_some_and(|r| !r.is_empty()) {
            self.exit_action = Some(ExitAction::PrintResult);
        }
        true
    }

    /// Open the save popup (`Ctrl+W`) with the default filename (`<stem>-out.csv`) prefilled.
    /// A no-op without a displayed result — there is nothing to save. Closes the other popups so
    /// overlays never stack.
    pub(crate) fn open_save(&mut self) {
        if self.display_rows().is_none() {
            return;
        }
        self.autocomplete.close();
        self.palette_open = false;
        self.close_facet();
        self.close_history();
        self.ai.close();
        let stem = self.source_stem.as_deref().unwrap_or("result");
        self.save.open(&format!("{stem}-out.csv"));
        self.refresh_save_preview();
    }

    /// Close the save popup without writing.
    pub(crate) fn close_save(&mut self) {
        self.save.close();
    }

    /// Handle a key while the save popup is open (it captures the keyboard, like the other
    /// popups): typing edits the filename (preview updates live), Enter resolves + writes,
    /// Esc closes, Ctrl-C still quits. Returns `true` if the app should quit.
    pub(crate) fn handle_save_key(&mut self, ev: &KeyEvent) -> bool {
        if ev.is_quit() {
            return true;
        }
        match ev.key {
            Key::Esc => self.close_save(),
            Key::Enter => self.write_save(),
            Key::Backspace => {
                self.save.pop();
                self.refresh_save_preview();
            }
            Key::Char(c) if !ev.mods.ctrl && !ev.mods.alt => {
                self.save.push(c);
                self.refresh_save_preview();
            }
            _ => {}
        }
        false
    }

    /// Recompute the resolved-path preview for the current filename (`None` while empty or
    /// unresolvable — the popup falls back to its hint line). The existence probe is what powers
    /// the overwrite warning.
    fn refresh_save_preview(&mut self) {
        let preview = save_io::resolve(self.save.filename(), self.save_home.as_deref())
            .ok()
            .map(|path| PathPreview {
                exists: path.exists(),
                path,
            });
        self.save.set_preview(preview);
    }

    /// Resolve the typed filename and write the displayed result as RFC-4180 CSV. On success the
    /// popup closes and the status line reports the destination; on failure the popup stays open
    /// with an inline error so the user can fix the name and retry.
    fn write_save(&mut self) {
        let path = match save_io::resolve(self.save.filename(), self.save_home.as_deref()) {
            Ok(p) => p,
            Err(e) => {
                self.save.set_error(e);
                return;
            }
        };
        let Some(rows) = self.display_rows() else {
            self.close_save();
            return;
        };
        let csv = render_output(rows, &rows.schema(), OutputFormat::Csv);
        let row_count = rows.row_count();
        match save_io::write(&path, &csv) {
            Ok(()) => {
                self.close_save();
                self.set_status(format!(
                    "saved {row_count} row{} to {}",
                    if row_count == 1 { "" } else { "s" },
                    path.display()
                ));
            }
            Err(e) => self.save.set_error(e),
        }
    }
}
