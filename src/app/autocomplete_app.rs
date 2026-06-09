//! Autocomplete App orchestration (`dev/PLAN.md` §5, P3.6/P3.7) — an `impl App` block lifted out
//! of `app.rs` to keep that file under the 1000-line cap, and because it is cohesive: recompute the
//! popup from the bar + cursor against the loaded schema, fetch distinct values out-of-band through
//! the worker when the cursor enters a value position, and extract the values from a value-fetch
//! result.
//!
//! Pane-aware (Simple mode): when the form is in Simple mode the orchestrator reads the focused
//! pane's text + cursor and synthesizes the governing clause prefix
//! ([`pane_context::pane_suggestions`]) so the same `clause_context::detect_context` /
//! `candidates::get_suggestions` pipeline drives the popup off a single pane's text. Power mode
//! continues to read the editor directly (the §1 single-buffer SQL).
//!
//! All of it is headless: the popup recompute is a pure function of `(query, cursor, schema,
//! value_cache)`; the only side effect is dispatching a value-completion fetch on the **same**
//! worker channel a grid query uses (§5.5 — autocomplete never opens its own engine connection).

use crate::app::App;
use crate::app::query_form::{QueryMode, SimplePane};
use crate::autocomplete::candidates::get_suggestions;
use crate::autocomplete::clause_context::{CursorContext, detect_context};
use crate::autocomplete::pane_context::{pane_context, pane_suggestions, synthesized_prefix};
use crate::autocomplete::sql_keywords::OPERATORS;
use crate::autocomplete::value_source::build_distinct_sql_default;
use crate::schema::Schema;
use crate::sql_lexer::tokenize;

impl App {
    /// Recompute the autocomplete popup from the current query + cursor against the loaded schema,
    /// and (P3.7) fetch distinct values through the worker when the cursor is in a value position
    /// for a column not yet cached. Closes the popup when there is no schema (still loading) or no
    /// candidate applies. Pure except for the out-of-band value fetch.
    ///
    /// Simple mode reads the focused pane via [`pane_context::pane_suggestions`] so each pane's
    /// candidate set is correct without the user having to type the clause keyword themselves.
    /// Power mode reads the editor directly.
    pub(crate) fn refresh_autocomplete(&mut self) {
        if self.schema.is_none() {
            self.autocomplete.close();
            return;
        }
        let suggestions = match self.query_form.mode() {
            QueryMode::Simple => self.refresh_simple_mode(),
            QueryMode::Power => self.refresh_power_mode(),
        };
        self.autocomplete.open_with(suggestions);
    }

    /// Power-mode refresh: the editor's text + cursor go through the single-buffer pipeline. Issues
    /// a value-fetch when the cursor is in a `ColumnValue` position for an uncached column.
    fn refresh_power_mode(&mut self) -> Vec<crate::autocomplete::autocomplete_state::Suggestion> {
        let query = self.query_form.power().text();
        let cursor = self.query_form.power().cursor_byte();
        // Resolve the value-fetch column under an immutable borrow of `schema`, then drop the
        // borrow before issuing the dispatch (which borrows `self` mutably) — splitting the borrows
        // keeps the same single source of `schema` without a clone.
        let col = self.schema.as_ref().and_then(|schema| {
            self.value_column_to_fetch_power(&query, cursor, schema)
                .map(|c| (c, schema.clone()))
        });
        if let Some((col, _)) = &col {
            self.dispatch_value_fetch(col.clone());
        }
        // Re-borrow schema immutably for the candidate generator. `unwrap` is safe — `refresh_autocomplete`
        // gates on `schema.is_some()` before calling either mode's refresh.
        let schema = self.schema.as_ref().expect("schema gated by caller");
        get_suggestions(&query, cursor, schema, OPERATORS, &self.value_cache)
    }

    /// Simple-mode refresh: read the focused pane and run the synthesized-prefix pipeline. The
    /// LIMIT pane returns no suggestions ([`pane_suggestions`] short-circuits there). Issues a
    /// value-fetch on the same worker channel when the focused pane's cursor lands in a value
    /// position — the synthesized prefix's byte length is subtracted out so the column key is the
    /// real schema column, never the prefix.
    fn refresh_simple_mode(&mut self) -> Vec<crate::autocomplete::autocomplete_state::Suggestion> {
        let pane = self.query_form.focused_pane();
        let pane_editor = self.query_form.pane(pane);
        let pane_text = pane_editor.text();
        let pane_cursor = pane_editor.cursor_byte();
        let col = self.schema.as_ref().and_then(|schema| {
            self.value_column_to_fetch_simple(pane, &pane_text, pane_cursor, schema)
        });
        if let Some(c) = col {
            self.dispatch_value_fetch(c);
        }
        let schema = self.schema.as_ref().expect("schema gated by caller");
        pane_suggestions(
            pane,
            &pane_text,
            pane_cursor,
            schema,
            OPERATORS,
            &self.value_cache,
        )
    }

    /// Issue an out-of-band distinct-values fetch for `column` on the worker channel. Same lane the
    /// grid query uses (§5.5 — no separate engine connection); the response routes to
    /// [`ValueCache`](crate::autocomplete::value_source::ValueCache) and the next refresh picks the
    /// values up.
    fn dispatch_value_fetch(&mut self, column: String) {
        let sql = build_distinct_sql_default(&column);
        let _ = self.dispatcher.dispatch_value(sql, column);
    }

    /// The column whose distinct values should be fetched now (Power mode): `Some(canonical_name)`
    /// when the editor's cursor is in a `ColumnValue` context for a known schema column not yet
    /// cached; `None` otherwise.
    ///
    /// The detected column text keeps the user's casing (`STATUS`), but DuckDB resolves unquoted
    /// identifiers case-insensitively, so we resolve to the canonical header spelling (`status`)
    /// and key the fetch + cache by it — keeping the fetch key, the cache key, and the candidate
    /// generator's lookup all in lockstep (see [`Schema::column_ci`]).
    fn value_column_to_fetch_power(
        &self,
        query: &str,
        cursor: usize,
        schema: &Schema,
    ) -> Option<String> {
        let tokens = tokenize(query);
        let CursorContext::ColumnValue { col, .. } = detect_context(query, &tokens, cursor) else {
            return None;
        };
        self.canonical_uncached(&col, schema)
    }

    /// Same as [`value_column_to_fetch_power`] for Simple mode: synthesize the pane's prefix, run
    /// the detector, and key the fetch by the canonical schema column. Returns `None` for the
    /// LIMIT pane (no completion) or when the cursor isn't in a value position.
    fn value_column_to_fetch_simple(
        &self,
        pane: SimplePane,
        pane_text: &str,
        pane_cursor: usize,
        schema: &Schema,
    ) -> Option<String> {
        // pane_context::pane_context handles the synthesized-prefix bookkeeping — keep this branch
        // free of prefix-length arithmetic so the offset never drifts from the synthesizer.
        let _ = synthesized_prefix(pane)?; // short-circuits LIMIT to None
        let CursorContext::ColumnValue { col, .. } = pane_context(pane, pane_text, pane_cursor)?
        else {
            return None;
        };
        self.canonical_uncached(&col, schema)
    }

    /// Resolve a possibly-mixed-case column reference to its canonical schema name and, if it's
    /// not already in the value cache, return it as the fetch key. Shared between the Power and
    /// Simple value-fetch paths so the resolution rule lives in one place.
    fn canonical_uncached(&self, col: &str, schema: &Schema) -> Option<String> {
        let canonical = &schema.column_ci(col)?.name;
        if self.value_cache.contains(canonical) {
            None
        } else {
            Some(canonical.clone())
        }
    }
}

/// Extract the distinct value strings from a value-fetch result table — the **first** column (the
/// `build_distinct_sql` shape is `SELECT "<col>", count(*) ...`, so column 0 holds the values, in
/// the frequency order the query produced). NULLs are already filtered by the query; any that slip
/// through render as the empty string and are skipped (a NULL is not a completable value).
pub(crate) fn distinct_values(table: &crate::engine::Table) -> Vec<String> {
    let Some(col) = table.columns().first() else {
        return Vec::new();
    };
    col.cells
        .iter()
        .filter(|c| !c.is_null())
        .map(|c| c.display())
        .collect()
}
