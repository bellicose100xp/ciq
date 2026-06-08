//! Palette state machine — the generated-state column picker's structured state (`dev/PLAN.md`
//! §6.2, `dev/DECISIONS.md` D3).
//!
//! Pure owned data with pure transitions: which columns are checked (in selection order — that
//! order drives the `SELECT` projection order), the conjunction of facet predicates, the fuzzy
//! filter needle, and the cursor into the filtered list. No terminal, no engine, no clock — every
//! transition is `&mut self` over plain data and is unit-tested with plain asserts.
//!
//! **Ordered-unique selection without a new dependency.** The plan calls for an `IndexSet<usize>`
//! to hold the checked columns (ordered + unique). Rather than add the `indexmap` crate for one
//! field, ciq models it as a `Vec<usize>` with an explicit membership check on insert: pushing an
//! already-present index is a no-op (uniqueness), and the push order is the projection order
//! (insertion order). Toggling off removes by value, preserving the relative order of the rest. The
//! set is small (column count), so the linear membership scan is free. This is the documented
//! "prefer no new dep" choice from the task brief.
//!
//! **Ownership is a byte-compare, never a parse (§0/D3).** [`PaletteState::owns`] compares the
//! current bar text against the last string the emitter produced. Equal -> the palette owns the
//! query and its edits stay live; different -> the user hand-typed SQL and the palette is disabled
//! (the App offers a soft "Replace?"). No SQL parsing anywhere.

use crate::schema::{ColumnType, Schema};
use crate::text_match::is_subsequence;

/// One column the palette can pick: its name (verbatim header text) and sniffed [`ColumnType`].
/// A lightweight owned snapshot of a [`crate::schema::ColumnMeta`] so the palette state is
/// self-contained (it does not borrow the `Schema` for its lifetime).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRef {
    /// The column name, exactly as the CSV header spells it.
    pub name: String,
    /// The sniffed column type (drives the type badge in the popup + value quoting in the emit).
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

/// A facet predicate operator — the comparison a [`Predicate`] applies. Deliberately a small,
/// closed set covering the facet/quick-filter affordances the palette offers; richer SQL is the
/// user's to hand-type (at which point the palette disables itself, §0/D3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredicateOp {
    /// `col = value` (or `col IS NULL` when the value is [`Predicate::is_null`]).
    Eq,
    /// `col != value` (or `col IS NOT NULL`).
    Neq,
    /// `col < value`.
    Lt,
    /// `col <= value`.
    Le,
    /// `col > value`.
    Gt,
    /// `col >= value`.
    Ge,
    /// `col LIKE value` (value emitted as a string literal).
    Like,
}

/// One facet predicate in the palette's generated `WHERE` conjunction: a column, an operator, and a
/// value. The value's quoting on emit is decided by the column's [`ColumnType`] (numeric/bool bare,
/// text/temporal single-quoted) and by [`is_null`](Self::is_null) (which becomes `IS NULL` /
/// `IS NOT NULL`). Holding the column's type here keeps [`query_emit`](super::query_emit) a pure
/// `state -> String` with no `Schema` lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Predicate {
    /// The column the predicate constrains (verbatim header name).
    pub column: String,
    /// The column's type — decides value quoting on emit.
    pub ty: ColumnType,
    /// The comparison operator.
    pub op: PredicateOp,
    /// The comparison value as the user entered it (unquoted, unescaped). `None` means a NULL test
    /// (`IS NULL` for [`PredicateOp::Eq`], `IS NOT NULL` for [`PredicateOp::Neq`]).
    pub value: Option<String>,
}

impl Predicate {
    /// A value predicate `col <op> value`.
    pub fn new(
        column: impl Into<String>,
        ty: ColumnType,
        op: PredicateOp,
        value: impl Into<String>,
    ) -> Self {
        Self {
            column: column.into(),
            ty,
            op,
            value: Some(value.into()),
        }
    }

    /// A NULL-test predicate (`col IS NULL` for `Eq`, `col IS NOT NULL` for `Neq`).
    pub fn null_test(column: impl Into<String>, ty: ColumnType, op: PredicateOp) -> Self {
        Self {
            column: column.into(),
            ty,
            op,
            value: None,
        }
    }

    /// Whether this is a NULL test (no value -> `IS [NOT] NULL`).
    pub fn is_null(&self) -> bool {
        self.value.is_none()
    }
}

/// The palette's structured state (`dev/PLAN.md` §6.2): the column universe, the ordered set of
/// checked column indices (the projection order), the facet-predicate conjunction, the fuzzy
/// filter needle, the cursor into the filtered view, and the last string the emitter produced (for
/// the ownership byte-compare).
///
/// All fields are private; transitions go through the methods so the invariants hold (checked
/// indices stay unique + in range; the cursor stays inside the filtered list).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaletteState {
    /// Every column in table order — the pick universe.
    all_columns: Vec<ColumnRef>,
    /// Checked column indices in **selection order** (drives the `SELECT` projection order).
    /// Ordered-unique: see the module docs for the no-new-dep `IndexSet` model.
    checked: Vec<usize>,
    /// The facet-predicate conjunction (`WHERE p1 AND p2 AND …`), in insertion order.
    predicates: Vec<Predicate>,
    /// The fuzzy filter needle (lowercased subsequence match against column names).
    needle: String,
    /// The cursor into the **filtered** column list (the currently highlighted row).
    cursor: usize,
    /// The last string [`query_emit::emit`](super::query_emit::emit) produced for this state, set by
    /// [`record_emitted`](Self::record_emitted). The ownership check (§0/D3) byte-compares the bar
    /// against this. `None` until the first emit.
    last_emitted: Option<String>,
}

impl PaletteState {
    /// Build a palette over a column universe (table order). Nothing checked, no predicates, empty
    /// needle, cursor at the top.
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

    /// Every column in the pick universe (table order).
    pub fn all_columns(&self) -> &[ColumnRef] {
        &self.all_columns
    }

    /// The checked column indices, in selection (projection) order.
    pub fn checked(&self) -> &[usize] {
        &self.checked
    }

    /// The checked columns resolved to [`ColumnRef`]s, in selection order — the projection the
    /// emitter renders. Skips any stale index (none can occur through the public API, but the
    /// resolve stays total).
    pub fn checked_columns(&self) -> Vec<&ColumnRef> {
        self.checked
            .iter()
            .filter_map(|&i| self.all_columns.get(i))
            .collect()
    }

    /// The facet predicates, in insertion order.
    pub fn predicates(&self) -> &[Predicate] {
        &self.predicates
    }

    /// The current fuzzy filter needle.
    pub fn needle(&self) -> &str {
        &self.needle
    }

    /// The cursor into the filtered column list.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Whether column index `i` is currently checked.
    pub fn is_checked(&self, i: usize) -> bool {
        self.checked.contains(&i)
    }

    /// The last string the emitter produced for this state, if any.
    pub fn last_emitted(&self) -> Option<&str> {
        self.last_emitted.as_deref()
    }

    // --- filtered view (the fuzzy needle decides which rows show) ---

    /// The indices (into [`all_columns`](Self::all_columns)) of the columns matching the current
    /// needle, in table order. An empty needle matches every column. The match is a
    /// case-insensitive subsequence (the shared [`crate::text_match::is_subsequence`] — the same
    /// rule the autocomplete ranker uses), so a needle never reorders the universe — it only filters
    /// it (the determinism stable-order rule).
    pub fn filtered_indices(&self) -> Vec<usize> {
        if self.needle.is_empty() {
            return (0..self.all_columns.len()).collect();
        }
        let needle = self.needle.to_ascii_lowercase();
        self.all_columns
            .iter()
            .enumerate()
            .filter(|(_, c)| is_subsequence(&c.name.to_ascii_lowercase(), &needle))
            .map(|(i, _)| i)
            .collect()
    }

    /// The column at the cursor in the filtered view, if the filtered list is non-empty.
    pub fn cursor_column_index(&self) -> Option<usize> {
        self.filtered_indices().get(self.cursor).copied()
    }

    // --- transitions (pure `&mut self`) ---

    /// Toggle the checked state of column index `i`. Checking appends it to the selection order (so
    /// the projection lists columns in the order the user checked them); unchecking removes it,
    /// preserving the relative order of the rest. Out-of-range `i` is ignored.
    pub fn toggle(&mut self, i: usize) {
        if i >= self.all_columns.len() {
            return;
        }
        if let Some(pos) = self.checked.iter().position(|&c| c == i) {
            self.checked.remove(pos);
        } else {
            self.checked.push(i);
        }
    }

    /// Toggle the column currently under the cursor (the filtered-view highlight). No-op when the
    /// filtered list is empty.
    pub fn toggle_cursor(&mut self) {
        if let Some(i) = self.cursor_column_index() {
            self.toggle(i);
        }
    }

    /// Move a checked column one position **earlier** in the projection order (toward the front of
    /// the `SELECT` list). Identified by its column index `i`; no-op if `i` is not checked or
    /// already first.
    pub fn move_selection_up(&mut self, i: usize) {
        if let Some(pos) = self.checked.iter().position(|&c| c == i)
            && pos > 0
        {
            self.checked.swap(pos, pos - 1);
        }
    }

    /// Move a checked column one position **later** in the projection order (toward the end of the
    /// `SELECT` list). No-op if `i` is not checked or already last.
    pub fn move_selection_down(&mut self, i: usize) {
        if let Some(pos) = self.checked.iter().position(|&c| c == i)
            && pos + 1 < self.checked.len()
        {
            self.checked.swap(pos, pos + 1);
        }
    }

    /// Move the cursor down one row in the filtered view, wrapping from the last back to the first.
    /// No-op when the filtered list is empty.
    pub fn cursor_down(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            return;
        }
        self.cursor = (self.cursor + 1) % len;
    }

    /// Move the cursor up one row in the filtered view, wrapping from the first to the last. No-op
    /// when the filtered list is empty.
    pub fn cursor_up(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            return;
        }
        self.cursor = if self.cursor == 0 {
            len - 1
        } else {
            self.cursor - 1
        };
    }

    /// Move the cursor to row `row` in the filtered view (the mouse click-to-select path), clamped
    /// to the last filtered row. No-op when the filtered list is empty.
    pub fn set_cursor(&mut self, row: usize) {
        let len = self.filtered_indices().len();
        if len == 0 {
            return;
        }
        self.cursor = row.min(len - 1);
    }

    /// Append a character to the fuzzy filter needle and clamp the cursor back into the (now
    /// possibly shorter) filtered list.
    pub fn push_needle(&mut self, c: char) {
        self.needle.push(c);
        self.clamp_cursor();
    }

    /// Remove the last character from the needle and clamp the cursor into the filtered list.
    pub fn pop_needle(&mut self) {
        self.needle.pop();
        self.clamp_cursor();
    }

    /// Replace the whole needle (and clamp the cursor).
    pub fn set_needle(&mut self, needle: impl Into<String>) {
        self.needle = needle.into();
        self.clamp_cursor();
    }

    /// Add a facet predicate to the conjunction.
    pub fn add_predicate(&mut self, p: Predicate) {
        self.predicates.push(p);
    }

    /// Remove the facet predicate at `idx` (no-op if out of range).
    pub fn remove_predicate(&mut self, idx: usize) {
        if idx < self.predicates.len() {
            self.predicates.remove(idx);
        }
    }

    /// Record the string the emitter just produced, so a later [`owns`](Self::owns) byte-compare
    /// can tell whether the bar still holds the palette's own emission (§0/D3).
    pub fn record_emitted(&mut self, sql: impl Into<String>) {
        self.last_emitted = Some(sql.into());
    }

    /// Whether the palette **owns** `bar_text` — i.e. it byte-equals the last string the emitter
    /// produced. Equal -> palette-owned (its edits stay live); different (or never emitted) -> the
    /// user hand-typed SQL and the palette is disabled (§0/D3). A pure byte-compare; **no parsing**.
    pub fn owns(&self, bar_text: &str) -> bool {
        self.last_emitted.as_deref() == Some(bar_text)
    }

    /// Clamp the cursor so it stays inside the current filtered list (used after a needle edit
    /// shrinks the list). An empty list parks the cursor at 0.
    fn clamp_cursor(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.cursor = 0;
        } else if self.cursor >= len {
            self.cursor = len - 1;
        }
    }
}

#[cfg(test)]
#[path = "palette_state_tests.rs"]
mod palette_state_tests;
