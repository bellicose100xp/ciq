//! The Simple/Power query form — five labeled clause panes (Simple) or one full-SQL textarea
//! (Power), toggled by `Ctrl+Q`.
//!
//! `dev/PLAN.md` post-5 UX redesign (Stage 1 foundation). The Simple-mode panes are five thin
//! [`Editor`] wrappers (one per clause) that the App renders as five stacked single-line rows in
//! the bordered query box. The composer ([`composer::compose_sql`]) projects the five pane texts
//! onto the dispatched SQL on each debounce; the simplifier ([`simplifier::try_simplify_from_sql`])
//! converts a Power-mode SQL string back into pane texts when the user toggles Power -> Simple.
//!
//! This module is the **state container** — it owns the five Simple panes, the Power editor, the
//! current mode, the focused pane, and the most recent LIMIT-pane validation error. The App owns
//! the routing and the dispatch loop; this struct is pure data + a few convenience accessors.

use crate::app::editor::{Editor, EditorMode};

pub mod composer;
pub mod simplifier;

pub use composer::{ComposeError, compose_sql, invalid_limit_message};
pub use simplifier::{SimpleParts, SimplifyError, try_simplify_from_sql};

/// Which mode the query form is in.
///
/// * `Simple` — five labeled stacked single-line clause panes (`SELECT` / `WHERE` / `GROUP BY` /
///   `ORDER BY` / `LIMIT`). The composer projects these onto the dispatched SQL.
/// * `Power` — a single multiline textarea with the full SQL (the original ciq query bar). The
///   user types raw SQL; the dispatcher uses it verbatim.
///
/// `Ctrl+Q` toggles the two: Simple -> Power composes the current pane texts into the Power
/// textarea; Power -> Simple parses the textarea via the simplifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueryMode {
    /// The five-pane labeled form (default on launch).
    #[default]
    Simple,
    /// The full-SQL multiline textarea (`Ctrl+Q` toggle).
    Power,
}

/// Which Simple-mode pane currently has the focus / cursor. Cycles via `Tab`/`Shift+Tab`; on the
/// LIMIT pane, `Down` hands focus off to the results grid (mirroring the single-line bar's
/// `Down`-to-Results handoff).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimplePane {
    Select,
    Where,
    GroupBy,
    OrderBy,
    Limit,
}

impl SimplePane {
    /// All five panes, in the canonical top-to-bottom order. Used by Tab/Shift+Tab and by the
    /// render layer to stack the rows.
    pub const ALL: [SimplePane; 5] = [
        SimplePane::Select,
        SimplePane::Where,
        SimplePane::GroupBy,
        SimplePane::OrderBy,
        SimplePane::Limit,
    ];

    /// 0-based index in `ALL` (the row index in the bordered box). Mouse click-to-focus uses this
    /// to map a clicked row to a pane.
    pub fn index(self) -> usize {
        match self {
            SimplePane::Select => 0,
            SimplePane::Where => 1,
            SimplePane::GroupBy => 2,
            SimplePane::OrderBy => 3,
            SimplePane::Limit => 4,
        }
    }

    /// The pane at row index `i` (0..5), or `None` for an out-of-range index.
    pub fn from_index(i: usize) -> Option<SimplePane> {
        Self::ALL.get(i).copied()
    }

    /// The static label rendered to the left of the pane's editor row (`"SELECT"`, `"WHERE"`,
    /// `"GROUP BY"`, `"ORDER BY"`, `"LIMIT"`).
    pub fn label(self) -> &'static str {
        match self {
            SimplePane::Select => "SELECT",
            SimplePane::Where => "WHERE",
            SimplePane::GroupBy => "GROUP BY",
            SimplePane::OrderBy => "ORDER BY",
            SimplePane::Limit => "LIMIT",
        }
    }
}

/// The Simple/Power query form. Owns the five pane editors, the Power editor, the current mode,
/// the focused pane, and the last LIMIT-pane validation error (used by the App for the status line).
pub struct QueryForm {
    mode: QueryMode,
    panes: [Editor; 5],
    focused_pane: SimplePane,
    power: Editor,
    /// The last LIMIT-pane validation error, or `None` when LIMIT is currently valid. Refreshed
    /// on every dispatch attempt; the App reads it to set the status line.
    limit_error: Option<String>,
}

impl Default for QueryForm {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a fresh form already in Power mode. The App's input loop is still wired to the
/// single-buffer `editor` (the Power editor's stand-in), so the form starts in Power mode to keep
/// behavior unchanged until per-pane input routing lands. The autocomplete pipeline reads the
/// form's mode + focused pane to compute pane-aware suggestions when (and only when) Simple mode
/// is active.
pub fn power_default() -> QueryForm {
    let mut form = QueryForm::new();
    form.mode = QueryMode::Power;
    form
}

impl QueryForm {
    /// Build a fresh form: Simple mode, panes seeded `SELECT="*"`, `WHERE=""` (focused),
    /// `GROUP BY=""`, `ORDER BY=""`, `LIMIT="1000"`. The App overrides the LIMIT seed with the
    /// configured `[general] row_limit` via [`set_default_limit_seed`](Self::set_default_limit_seed).
    pub fn new() -> Self {
        let panes = [
            Editor::with_text("*"),
            Editor::new(),
            Editor::new(),
            Editor::new(),
            Editor::with_text("1000"),
        ];
        Self {
            mode: QueryMode::Simple,
            panes,
            focused_pane: SimplePane::Where,
            power: Editor::new(),
            limit_error: None,
        }
    }

    /// Re-seed the LIMIT pane from a configured default (the `[general] row_limit` once the App
    /// reads it). Only re-seeds when the LIMIT pane is still the construction-default `1000`, so
    /// a user who has typed something into LIMIT won't be clobbered. No-op in Power mode.
    pub fn set_default_limit_seed(&mut self, default_limit: usize) {
        let limit = &mut self.panes[SimplePane::Limit.index()];
        if limit.text() == "1000" {
            limit.set_text(default_limit.to_string());
        }
    }

    /// The current mode (`Simple` or `Power`).
    pub fn mode(&self) -> QueryMode {
        self.mode
    }

    /// The currently focused Simple pane (relevant only in `Simple` mode; in Power mode the
    /// focus is implicitly the Power editor).
    pub fn focused_pane(&self) -> SimplePane {
        self.focused_pane
    }

    /// The latest LIMIT-pane validation message, or `None` when LIMIT is valid. The App lifts
    /// this onto the status line.
    pub fn limit_error(&self) -> Option<&str> {
        self.limit_error.as_deref()
    }

    /// Set or clear the LIMIT-pane validation message (the App calls this from its dispatch path
    /// when the composer's `LIMIT` parse fails).
    pub fn set_limit_error(&mut self, msg: Option<String>) {
        self.limit_error = msg;
    }

    /// The editor backing the focused surface — the focused Simple pane in Simple mode, the
    /// Power editor in Power mode. The render layer (and the existing autocomplete /
    /// click-to-position seam) drives this single editor instance.
    pub fn focused_editor(&self) -> &Editor {
        match self.mode {
            QueryMode::Simple => &self.panes[self.focused_pane.index()],
            QueryMode::Power => &self.power,
        }
    }

    /// Mutable access to the focused surface's editor — see [`focused_editor`](Self::focused_editor).
    pub fn focused_editor_mut(&mut self) -> &mut Editor {
        match self.mode {
            QueryMode::Simple => &mut self.panes[self.focused_pane.index()],
            QueryMode::Power => &mut self.power,
        }
    }

    /// Borrow a Simple pane's editor by enum (any pane, regardless of focus). Used by the App's
    /// composer call and tests.
    pub fn pane(&self, pane: SimplePane) -> &Editor {
        &self.panes[pane.index()]
    }

    /// Mutable access to a Simple pane's editor. Used by the simplifier load path and tests.
    pub fn pane_mut(&mut self, pane: SimplePane) -> &mut Editor {
        &mut self.panes[pane.index()]
    }

    /// The Power-mode editor (the full-SQL multiline textarea).
    pub fn power(&self) -> &Editor {
        &self.power
    }

    /// Mutable access to the Power-mode editor.
    pub fn power_mut(&mut self) -> &mut Editor {
        &mut self.power
    }

    /// The text currently in `pane` (clone of the editor's joined string).
    pub fn text(&self, pane: SimplePane) -> String {
        self.panes[pane.index()].text()
    }

    /// Replace `pane`'s text wholesale. Resets the editor to Insert mode (the wholesale-set
    /// invariant in [`Editor::set_text`]).
    pub fn set_text(&mut self, pane: SimplePane, text: impl Into<String>) {
        self.panes[pane.index()].set_text(text);
    }

    /// Move focus to the next Simple pane (cycling). No-op in Power mode.
    pub fn focus_next(&mut self) {
        if self.mode != QueryMode::Simple {
            return;
        }
        let next = (self.focused_pane.index() + 1) % SimplePane::ALL.len();
        self.set_focus(SimplePane::ALL[next]);
    }

    /// Move focus to the previous Simple pane (cycling). No-op in Power mode.
    pub fn focus_prev(&mut self) {
        if self.mode != QueryMode::Simple {
            return;
        }
        let len = SimplePane::ALL.len();
        let prev = (self.focused_pane.index() + len - 1) % len;
        self.set_focus(SimplePane::ALL[prev]);
    }

    /// Set focus to `pane` (the click-to-focus path). Resets the new pane's editor to Insert
    /// mode so typing resumes immediately. No-op in Power mode.
    pub fn focus(&mut self, pane: SimplePane) {
        if self.mode != QueryMode::Simple {
            return;
        }
        self.set_focus(pane);
    }

    /// Internal: set the focused pane and reset its editor to Insert mode (the click/focus
    /// invariant — typing resumes after a focus change).
    fn set_focus(&mut self, pane: SimplePane) {
        self.focused_pane = pane;
        self.panes[pane.index()].set_mode(EditorMode::Insert);
    }

    /// Compose the dispatched SQL from the current Simple pane texts (the composer's projection).
    /// `default_limit` is the App's `[general] row_limit` (used as the LIMIT-pane fallback).
    /// In Power mode, returns the Power editor's text verbatim.
    pub fn to_full_sql(&self, default_limit: usize) -> Result<String, ComposeError> {
        match self.mode {
            QueryMode::Simple => compose_sql(
                &self.panes[SimplePane::Select.index()].text(),
                &self.panes[SimplePane::Where.index()].text(),
                &self.panes[SimplePane::GroupBy.index()].text(),
                &self.panes[SimplePane::OrderBy.index()].text(),
                &self.panes[SimplePane::Limit.index()].text(),
                default_limit,
            ),
            QueryMode::Power => Ok(self.power.text()),
        }
    }

    /// Toggle Simple <-> Power. Composes the current Simple panes into the Power textarea on
    /// Simple -> Power; runs the simplifier on Power -> Simple and (on success) distributes the
    /// parsed SimpleParts into the panes (with the LIMIT pane defaulted to `default_limit` when
    /// the source had no LIMIT). Returns an `Err(SimplifyError)` only on a refused Power -> Simple
    /// (the form stays in Power); all other transitions return `Ok(())`.
    pub fn toggle_mode(&mut self, default_limit: usize) -> Result<(), SimplifyError> {
        match self.mode {
            QueryMode::Simple => {
                let composed = self
                    .to_full_sql(default_limit)
                    .unwrap_or_else(|_| format!("SELECT * FROM t LIMIT {default_limit}"));
                self.power.set_text(composed);
                self.mode = QueryMode::Power;
                self.limit_error = None;
                Ok(())
            }
            QueryMode::Power => {
                let parts = try_simplify_from_sql(&self.power.text())?;
                self.apply_parts(parts, default_limit);
                self.mode = QueryMode::Simple;
                self.focused_pane = SimplePane::Where;
                self.limit_error = None;
                Ok(())
            }
        }
    }

    /// Distribute parsed [`SimpleParts`] into the five Simple panes, defaulting the LIMIT pane
    /// to `default_limit.to_string()` when the source had no LIMIT.
    fn apply_parts(&mut self, parts: SimpleParts, default_limit: usize) {
        self.panes[SimplePane::Select.index()].set_text(parts.select);
        self.panes[SimplePane::Where.index()].set_text(parts.where_clause);
        self.panes[SimplePane::GroupBy.index()].set_text(parts.group_by);
        self.panes[SimplePane::OrderBy.index()].set_text(parts.order_by);
        let limit_text = if parts.limit.is_empty() {
            default_limit.to_string()
        } else {
            parts.limit
        };
        self.panes[SimplePane::Limit.index()].set_text(limit_text);
    }

    /// Force the form into Power mode, putting `sql` into the Power editor (the AI-accept path:
    /// the AI returns full SQL, so on accept we switch to Power and put the SQL there).
    pub fn enter_power_with_sql(&mut self, sql: impl Into<String>) {
        self.power.set_text(sql);
        self.mode = QueryMode::Power;
        self.limit_error = None;
    }

    /// Replace **all** five panes wholesale and force Simple mode (the palette-emit / palette-seed
    /// path: the palette emits a clean `SELECT … FROM t LIMIT n`, which the App distributes into
    /// the panes). Caller has already simplified the SQL into parts.
    pub fn enter_simple_with_parts(&mut self, parts: SimpleParts, default_limit: usize) {
        self.apply_parts(parts, default_limit);
        self.mode = QueryMode::Simple;
        self.focused_pane = SimplePane::Where;
        self.limit_error = None;
    }
}

#[cfg(test)]
#[path = "query_form_tests.rs"]
mod query_form_tests;
