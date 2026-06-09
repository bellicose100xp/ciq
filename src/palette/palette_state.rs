//! Column palette state — the SELECT-pane column picker (`dev/PLAN.md` §6.2 update; user-locked
//! redesign 2026-06-09).
//!
//! Pure state of "which schema columns are checked." Anchored to the SELECT pane: the popup is the
//! ONLY place ciq lets the user toggle a column projection, and **every toggle rewrites the SELECT
//! pane immediately** (the App layer, [`crate::app::palette_app`], wires this side effect — this
//! module stays pure). The popup is open or closed; there is no separate "accept" action and no
//! ownership byte-compare anymore (the SELECT pane *is* the source of truth, and this state mirrors
//! it for the duration of the popup).
//!
//! The checked set is a [`BTreeSet<usize>`] over schema indices — emit order is **schema order**,
//! not toggle order, so a click pattern can never reorder the projection. (Reordering is a future
//! affordance; out of scope for v1 of the live picker.)
//!
//! No fuzzy filter. The popup shows every schema column in order — selecting one is two keystrokes
//! (cursor + Space/Tab/Enter), filtering doesn't earn its complexity for a typical CSV's column
//! count. If a future CSV with hundreds of columns demands it, add a needle then.

use std::collections::BTreeSet;

use crate::schema::{ColumnType, Schema};
use crate::sql_ident::quote_ident_if_needed;
use crate::sql_lexer::{TokenKind, tokenize};

/// One column the picker can toggle: name (verbatim header text) and sniffed [`ColumnType`].
/// A self-contained snapshot of a [`crate::schema::ColumnMeta`] so the palette state owns its data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRef {
    pub name: String,
    pub ty: ColumnType,
}

impl ColumnRef {
    pub fn new(name: impl Into<String>, ty: ColumnType) -> Self {
        Self {
            name: name.into(),
            ty,
        }
    }
}

/// The palette popup's state: the column universe (schema order), which schema indices are checked,
/// and the cursor (the highlighted row). All transitions are pure `&mut self`; out-of-range or
/// empty-list operations are no-ops, never panics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaletteState {
    /// Every column in schema order — the pick universe.
    all_columns: Vec<ColumnRef>,
    /// Schema indices that are currently checked. `BTreeSet` so emit order is naturally schema order.
    checked: BTreeSet<usize>,
    /// The highlight row in `all_columns` (0-based). Bounded at `[0, len)`.
    cursor: usize,
}

impl PaletteState {
    /// Build a palette over a column universe (schema order). Nothing checked, cursor at the top.
    pub fn new(all_columns: Vec<ColumnRef>) -> Self {
        Self {
            all_columns,
            ..Self::default()
        }
    }

    /// Build a palette from a loaded [`Schema`] — the common path. Snapshots each column's name +
    /// type into a [`ColumnRef`] so the palette owns its data (no borrow of the schema).
    pub fn from_schema(schema: &Schema) -> Self {
        let cols = schema
            .columns()
            .iter()
            .map(|c| ColumnRef::new(&c.name, c.ty.clone()))
            .collect();
        Self::new(cols)
    }

    // --- read-only accessors ---

    /// Every column in the pick universe (schema order).
    pub fn all_columns(&self) -> &[ColumnRef] {
        &self.all_columns
    }

    /// The cursor row (0-based index into `all_columns`).
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Whether schema column index `i` is checked.
    pub fn is_checked(&self, i: usize) -> bool {
        self.checked.contains(&i)
    }

    /// The schema-index set (read-only). Visible for tests and for the popup row chrome to render
    /// `[x]` vs `[ ]`. Not part of the public API for non-test consumers.
    pub fn checked_set(&self) -> &BTreeSet<usize> {
        &self.checked
    }

    // --- transitions (pure `&mut self`) ---

    /// Replace the checked set so it matches `select_text`'s semantics:
    ///   * empty (or `*`) → every column checked;
    ///   * non-empty comma list → only the named columns checked (case-insensitive against the
    ///     schema; quoted idents `"order"` strip to `order` before comparison);
    ///   * a name that doesn't exist in the schema is silently ignored.
    ///
    /// Used by the App when opening the popup so it pre-checks against the live SELECT pane.
    pub fn open_with_select(&mut self, select_text: &str) {
        let trimmed = select_text.trim();
        self.cursor = 0;
        if trimmed.is_empty() || trimmed == "*" {
            self.checked = (0..self.all_columns.len()).collect();
            return;
        }
        let names = parse_select_list(trimmed);
        let mut next = BTreeSet::new();
        for n in names {
            let n_lc = n.to_ascii_lowercase();
            if let Some(idx) = self
                .all_columns
                .iter()
                .position(|c| c.name.to_ascii_lowercase() == n_lc)
            {
                next.insert(idx);
            }
            // unknown column names are silently dropped — no crash, no error.
        }
        self.checked = next;
    }

    /// Move the cursor down one row. **Bounded** — no wrap; at the bottom it is a no-op. Empty list
    /// is a no-op.
    pub fn cursor_down(&mut self) {
        let len = self.all_columns.len();
        if len == 0 {
            return;
        }
        if self.cursor + 1 < len {
            self.cursor += 1;
        }
    }

    /// Move the cursor up one row. **Bounded** — no wrap; at the top it is a no-op. Empty list is a
    /// no-op.
    pub fn cursor_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move the cursor to row `row`, clamped to the last row. Empty list is a no-op.
    pub fn set_cursor(&mut self, row: usize) {
        let len = self.all_columns.len();
        if len == 0 {
            return;
        }
        self.cursor = row.min(len - 1);
    }

    /// Toggle the checked state of schema index `i`. Out-of-range is ignored.
    pub fn toggle(&mut self, i: usize) {
        if i >= self.all_columns.len() {
            return;
        }
        if !self.checked.insert(i) {
            self.checked.remove(&i);
        }
    }

    /// Toggle the column under the cursor. Empty list is a no-op.
    pub fn toggle_at_cursor(&mut self) {
        if self.all_columns.is_empty() {
            return;
        }
        self.toggle(self.cursor);
    }

    /// Check every column.
    pub fn select_all(&mut self) {
        self.checked = (0..self.all_columns.len()).collect();
    }

    /// Uncheck every column.
    pub fn deselect_all(&mut self) {
        self.checked.clear();
    }

    /// Invert the checked set — checked becomes unchecked and vice versa.
    pub fn invert(&mut self) {
        let mut next = BTreeSet::new();
        for i in 0..self.all_columns.len() {
            if !self.checked.contains(&i) {
                next.insert(i);
            }
        }
        self.checked = next;
    }

    /// Render the checked set into the SELECT-pane text:
    ///   * all columns checked → `*`;
    ///   * subset checked → comma-separated names in **schema order**, each
    ///     [`quote_ident_if_needed`](crate::sql_ident::quote_ident_if_needed)-quoted;
    ///   * empty checked set → empty string (the composer falls back to `*`, so the grid keeps
    ///     showing the full table — the user-locked behavior).
    pub fn write_to_select(&self) -> String {
        if self.all_columns.is_empty() {
            return String::new();
        }
        if self.checked.len() == self.all_columns.len() {
            return "*".to_string();
        }
        if self.checked.is_empty() {
            return String::new();
        }
        let mut parts: Vec<String> = Vec::with_capacity(self.checked.len());
        for &i in self.checked.iter() {
            if let Some(c) = self.all_columns.get(i) {
                parts.push(quote_ident_if_needed(&c.name));
            }
        }
        parts.join(", ")
    }
}

/// Split a SELECT-pane projection list at top-level commas. Whitespace around each name is
/// trimmed; a `"quoted"` ident has its outer quotes stripped and its `""` doubled-quote escapes
/// collapsed to `"`. Names containing top-level commas inside parens (e.g. `func(a, b)`) are
/// preserved as a single name string — the App's column lookup will silently drop it as
/// "not a known schema column," which is the documented behavior for a non-trivial projection.
///
/// Reuses [`crate::sql_lexer::tokenize`] for paren-depth + string/quoted-ident tracking — D6's "no
/// parallel scanner" rule. The function is `pub(crate)` so tests can pin its behavior directly.
pub(crate) fn parse_select_list(s: &str) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }
    let toks = tokenize(s);
    // Find every top-level (paren-depth 0) comma; the names live in the byte ranges between them.
    let mut splits: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    for t in &toks {
        if t.depth == 0 && matches!(t.kind, TokenKind::Punct) && t.text(s) == "," {
            splits.push((start, t.start));
            start = t.end;
        }
    }
    splits.push((start, s.len()));

    splits
        .into_iter()
        .filter_map(|(a, b)| {
            let raw = &s[a..b];
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            Some(strip_outer_quotes(trimmed))
        })
        .collect()
}

/// If `s` is a `"…"` quoted identifier, strip the outer quotes and collapse `""` to `"`. Otherwise
/// return `s` verbatim. Tolerant: a half-quoted `"foo` returns `"foo` unchanged (the lookup will
/// fail to match a real column anyway).
fn strip_outer_quotes(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        let inner = &s[1..s.len() - 1];
        return inner.replace("\"\"", "\"");
    }
    s.to_string()
}

#[cfg(test)]
#[path = "palette_state_tests.rs"]
mod palette_state_tests;
