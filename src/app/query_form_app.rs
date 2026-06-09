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
}
