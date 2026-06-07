//! Column types — ciq's engine-agnostic view of a column's data type.
//!
//! Canonical per `dev/PLAN.md` §0/D2: the type name is **`ColumnType`** (not `SqlType`),
//! and it lives in top-level `src/schema/` so both `engine/` (which produces it) and the
//! pure consumers (`autocomplete`, `grid`, `schema_bar`, `facets`) can depend on it without
//! reaching into the engine module.
//!
//! This enum is deliberately a small, closed, engine-neutral set. The **engine impl owns the
//! mapping** from its native type strings to `ColumnType` (DECISIONS D2) — `ColumnType` itself
//! never parses a DuckDB (or DataFusion/Arrow) type grammar, which keeps this module a pure,
//! dependency-free leaf.

/// ciq's neutral classification of a column's data type.
///
/// Used for: result-grid alignment (numeric/temporal right-aligned, text left), typed
/// autocomplete hints, and facet-SQL shaping. Anything the engine sniffs that doesn't map
/// cleanly to a known kind becomes [`ColumnType::Other`] carrying the raw type name, so we
/// never lose information and never guess.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ColumnType {
    /// Integer-valued (DuckDB `BIGINT`/`INTEGER`/`SMALLINT`/`TINYINT`/`HUGEINT` and unsigned).
    Int,
    /// Floating / fixed-point (DuckDB `DOUBLE`/`REAL`/`DECIMAL`/`FLOAT`).
    Float,
    /// Boolean (`BOOLEAN`).
    Bool,
    /// Calendar date with no time component (`DATE`).
    Date,
    /// Timestamp / datetime (`TIMESTAMP`, `TIMESTAMPTZ`, `TIME`).
    Timestamp,
    /// Text (`VARCHAR`, `CHAR`, `TEXT`, and the all-varchar fallback).
    Text,
    /// Anything else the engine reported, preserved verbatim (e.g. `STRUCT`, `LIST`,
    /// `MAP`, `BLOB`, nested types). Carries the engine's raw type string.
    Other(String),
}

impl ColumnType {
    /// Whether values of this type are conventionally **right-aligned** in the grid
    /// (numbers and temporals) vs left-aligned (text and structured/other).
    ///
    /// This is the single source of truth for alignment-by-type; the grid layout
    /// (`grid/`) calls it rather than re-deciding per renderer.
    pub fn is_right_aligned(&self) -> bool {
        matches!(
            self,
            ColumnType::Int | ColumnType::Float | ColumnType::Date | ColumnType::Timestamp
        )
    }

    /// Whether this is a numeric type (integer or floating/fixed-point).
    pub fn is_numeric(&self) -> bool {
        matches!(self, ColumnType::Int | ColumnType::Float)
    }

    /// Whether this is a temporal type (date or timestamp/time).
    pub fn is_temporal(&self) -> bool {
        matches!(self, ColumnType::Date | ColumnType::Timestamp)
    }

    /// A short, stable badge string for the schema bar / autocomplete type hints.
    /// ASCII only (no emoji), per the theme conventions.
    pub fn badge(&self) -> &str {
        match self {
            ColumnType::Int => "int",
            ColumnType::Float => "num",
            ColumnType::Bool => "bool",
            ColumnType::Date => "date",
            ColumnType::Timestamp => "ts",
            ColumnType::Text => "txt",
            ColumnType::Other(_) => "oth",
        }
    }
}
