//! Schema — engine-agnostic description of the loaded CSV (columns + types).
//!
//! Canonical home per `dev/PLAN.md` §0/D2 (top-level `src/schema/`, sibling of `engine/`).
//! Pure data, no DuckDB dependency, no live connection: the engine *produces* a `Schema`
//! once at load (`QueryEngine::load -> Result<Schema, _>`); everyone else borrows `&Schema`
//! read-only. Holding no engine handle is exactly what lets the autocomplete candidate
//! generator, grid layout, schema bar, and facets stay pure, headless functions.
//!
//! `Schema` is session-stable: computed once per file load, never mutated by interactive
//! queries.
//!
//! Per ciq conventions (inherited from jiq): no `mod.rs`; the `ColumnType` enum lives in the
//! `schema/types.rs` submodule (declared below); `Schema`/`ColumnMeta` live here directly to
//! avoid a same-name inner module. Tests live in separate `{name}_tests.rs` files wired via
//! `#[path]`.

pub mod types;

pub use types::ColumnType;

/// One column of the loaded table: its name (as it appears in the CSV header, verbatim) and
/// ciq's neutral [`ColumnType`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMeta {
    /// The column name, exactly as the engine reported it (raw header text). Quoting for
    /// SQL emission is applied at use-site, not stored here.
    pub name: String,
    /// ciq's neutral type classification.
    pub ty: ColumnType,
}

impl ColumnMeta {
    pub fn new(name: impl Into<String>, ty: ColumnType) -> Self {
        Self {
            name: name.into(),
            ty,
        }
    }
}

/// The loaded table's schema: an ordered list of columns. Order matches the CSV's column
/// order (and thus `SELECT *` output order).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Schema {
    columns: Vec<ColumnMeta>,
}

impl Schema {
    /// Build a schema from an ordered list of columns.
    pub fn new(columns: Vec<ColumnMeta>) -> Self {
        Self { columns }
    }

    /// All columns, in table order.
    pub fn columns(&self) -> &[ColumnMeta] {
        &self.columns
    }

    /// Number of columns.
    pub fn len(&self) -> usize {
        self.columns.len()
    }

    /// Whether the schema has no columns.
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    /// Look up a column by exact name. Returns the first match (CSV headers *can* duplicate;
    /// duplicate-header policy is a deferred ingest decision — see PLAN.md Q3).
    pub fn column(&self, name: &str) -> Option<&ColumnMeta> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// Look up a column case-insensitively, falling back from an exact match. DuckDB resolves
    /// unquoted identifiers case-insensitively, so a query referencing `STATUS` against a `status`
    /// header is valid SQL; the autocomplete value path resolves columns through this so the
    /// distinct-value fetch/lookup keys off the canonical header spelling regardless of the casing
    /// the user typed. An exact match is preferred so distinct same-name-different-case headers
    /// (a degenerate but possible CSV) resolve to the one actually written.
    pub fn column_ci(&self, name: &str) -> Option<&ColumnMeta> {
        self.column(name).or_else(|| {
            self.columns
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(name))
        })
    }

    /// The column type for `name`, if present.
    pub fn column_type(&self, name: &str) -> Option<&ColumnType> {
        self.column(name).map(|c| &c.ty)
    }

    /// The column type for `name`, resolved case-insensitively (see [`column_ci`](Self::column_ci)).
    pub fn column_type_ci(&self, name: &str) -> Option<&ColumnType> {
        self.column_ci(name).map(|c| &c.ty)
    }

    /// Column names in table order — the candidate source for autocomplete / palette.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.columns.iter().map(|c| c.name.as_str())
    }
}

#[cfg(test)]
#[path = "schema/types_tests.rs"]
mod types_tests;

#[cfg(test)]
#[path = "schema/schema_tests.rs"]
mod schema_tests;
