//! Autocomplete App orchestration (`dev/PLAN.md` §5, P3.6/P3.7) — an `impl App` block lifted out
//! of `app.rs` to keep that file under the 1000-line cap, and because it is cohesive: recompute the
//! popup from the bar + cursor against the loaded schema, fetch distinct values out-of-band through
//! the worker when the cursor enters a value position, and extract the values from a value-fetch
//! result.
//!
//! All of it is headless: the popup recompute is a pure function of `(query, cursor, schema,
//! value_cache)`; the only side effect is dispatching a value-completion fetch on the **same**
//! worker channel a grid query uses (§5.5 — autocomplete never opens its own engine connection).

use crate::app::App;
use crate::autocomplete::candidates::get_suggestions;
use crate::autocomplete::clause_context::{CursorContext, detect_context};
use crate::autocomplete::sql_keywords::OPERATORS;
use crate::autocomplete::value_source::build_distinct_sql_default;
use crate::schema::Schema;
use crate::sql_lexer::tokenize;

impl App {
    /// Recompute the autocomplete popup from the current query + cursor against the loaded schema,
    /// and (P3.7) fetch distinct values through the worker when the cursor is in a value position
    /// for a column not yet cached. Closes the popup when there is no schema (still loading) or no
    /// candidate applies. Pure except for the out-of-band value fetch.
    pub(crate) fn refresh_autocomplete(&mut self) {
        let Some(schema) = self.schema.as_ref() else {
            self.autocomplete.close();
            return;
        };
        let query = self.editor.text();
        let cursor = self.editor.cursor_byte();

        // If the cursor is in a value position for an uncached, known column, fetch its distinct
        // values through the worker (same channel/engine — autocomplete never opens its own
        // connection, §5.5). The popup fills in once the response lands.
        if let Some(col) = self.value_column_to_fetch(&query, cursor, schema) {
            let sql = build_distinct_sql_default(&col);
            let _ = self.dispatcher.dispatch_value(sql, col);
        }

        let suggestions = get_suggestions(&query, cursor, schema, OPERATORS, &self.value_cache);
        self.autocomplete.open_with(suggestions);
    }

    /// The column whose distinct values should be fetched now: `Some(canonical_name)` when the
    /// cursor is in a `ColumnValue` context for a column present in `schema` and not already cached;
    /// `None` otherwise (no value position, unknown column, or already cached).
    ///
    /// The detected column text keeps the user's casing (`STATUS`), but DuckDB resolves unquoted
    /// identifiers case-insensitively, so we resolve to the canonical header spelling (`status`)
    /// and key the fetch + cache by it — keeping the fetch key, the cache key, and the candidate
    /// generator's lookup all in lockstep (see [`Schema::column_ci`]).
    fn value_column_to_fetch(&self, query: &str, cursor: usize, schema: &Schema) -> Option<String> {
        let tokens = tokenize(query);
        let CursorContext::ColumnValue { col, .. } = detect_context(query, &tokens, cursor) else {
            return None;
        };
        let canonical = &schema.column_ci(&col)?.name;
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
