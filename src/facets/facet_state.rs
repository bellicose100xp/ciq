//! Facet popup state — the focused column, its type, and the parsed [`FacetResult`] (`dev/PLAN.md`
//! §6.5).
//!
//! Pure owned data with pure transitions: which column the facet is for (and its [`ColumnType`],
//! which decides whether the result is a numeric **summary** or a string **histogram**), whether
//! the result has landed yet, and the parsed stats. No terminal, no engine, no clock — the App
//! fires the SQL through the worker ([`facet_query`](super::facet_query)) and feeds the response
//! [`Table`] to [`FacetState::apply_result`]; everything else is plain-assert unit-tested.
//!
//! **Result parsing is keyed by the queried column's type, not by guessing the `Table` shape** —
//! [`build_facet_sql`](super::facet_query::build_facet_sql) chose the shape from that type, so the
//! parser reads the same type to interpret the columns it gets back. This keeps the query and the
//! parse in lockstep (the same discipline the value-cache fetch keys its column by canonical name).

use crate::engine::{Cell, Table};
use crate::schema::ColumnType;

/// One bar of the string top-K histogram: a value and how many rows hold it. Ordered by the query
/// (`n DESC, value ASC`), so the order here is stable/deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetBar {
    /// The value (rendered to a string by the engine; a value is never NULL here — the histogram
    /// query filters `IS NOT NULL`).
    pub value: String,
    /// The number of rows holding `value`.
    pub count: u64,
}

impl FacetBar {
    pub fn new(value: impl Into<String>, count: u64) -> Self {
        Self {
            value: value.into(),
            count,
        }
    }
}

/// The parsed facet stats for a column — a numeric/temporal **summary** or a string **histogram**.
/// Both carry the column-wide distinct-count and null-count; the histogram adds the top-K bars and
/// the summary adds the min/max.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FacetResult {
    /// Numeric / temporal / bool: min, max, distinct count, null count.
    Summary {
        /// Rendered MIN (`None` when the column is entirely NULL — `min` returns NULL).
        min: Option<String>,
        /// Rendered MAX (`None` when the column is entirely NULL).
        max: Option<String>,
        /// Number of distinct non-null values.
        distinct: u64,
        /// Number of NULL rows.
        nulls: u64,
    },
    /// Text / other: the top-K most-frequent values (the bars), plus the column-wide distinct and
    /// null counts.
    Histogram {
        /// The most-frequent values, already ordered `count DESC, value ASC`.
        bars: Vec<FacetBar>,
        /// Number of distinct non-null values across the whole column (not just the shown top-K).
        distinct: u64,
        /// Number of NULL rows.
        nulls: u64,
    },
}

impl FacetResult {
    /// The column-wide distinct-value count.
    pub fn distinct(&self) -> u64 {
        match self {
            FacetResult::Summary { distinct, .. } | FacetResult::Histogram { distinct, .. } => {
                *distinct
            }
        }
    }

    /// The NULL-row count.
    pub fn nulls(&self) -> u64 {
        match self {
            FacetResult::Summary { nulls, .. } | FacetResult::Histogram { nulls, .. } => *nulls,
        }
    }

    /// The largest bar count, used to scale the histogram bar widths. `0` for a summary or an empty
    /// histogram (so the bar-width math divides by a safe value).
    pub fn max_count(&self) -> u64 {
        match self {
            FacetResult::Summary { .. } => 0,
            FacetResult::Histogram { bars, .. } => bars.iter().map(|b| b.count).max().unwrap_or(0),
        }
    }
}

/// The facet popup's state: the focused column, its type, and the result once it lands. `None`
/// result = the fetch is in-flight (the popup shows "computing…").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetState {
    column: String,
    ty: ColumnType,
    result: Option<FacetResult>,
}

impl FacetState {
    /// Open a facet for `column` of type `ty`, with no result yet (the SQL has been dispatched; the
    /// popup shows a pending state until [`apply_result`](Self::apply_result)).
    pub fn pending(column: impl Into<String>, ty: ColumnType) -> Self {
        Self {
            column: column.into(),
            ty,
            result: None,
        }
    }

    /// The column the facet is for (canonical header name).
    pub fn column(&self) -> &str {
        &self.column
    }

    /// The focused column's type (decides summary vs histogram parsing + the popup heading).
    pub fn column_type(&self) -> &ColumnType {
        &self.ty
    }

    /// The parsed result, or `None` while the fetch is still in-flight.
    pub fn result(&self) -> Option<&FacetResult> {
        self.result.as_ref()
    }

    /// Whether the result has landed (vs still computing).
    pub fn is_ready(&self) -> bool {
        self.result.is_some()
    }

    /// Parse a facet-query response `Table` into a [`FacetResult`] and store it. The shape is read
    /// from the focused column's type — exactly the type
    /// [`build_facet_sql`](super::facet_query::build_facet_sql) used to choose the SQL — so the two
    /// stay in lockstep. A `Table` that doesn't match (an engine/shape surprise) yields an empty
    /// result of the expected kind rather than panicking.
    pub fn apply_result(&mut self, table: &Table) {
        let result = if is_histogram_type(&self.ty) {
            parse_histogram(table)
        } else {
            parse_summary(table)
        };
        self.result = Some(result);
    }
}

/// Whether a type's facet is the string-histogram shape (mirrors `facet_query::is_histogram_type`,
/// kept here so the parse and the emit agree without either importing the other's private fn).
fn is_histogram_type(ty: &ColumnType) -> bool {
    matches!(ty, ColumnType::Text | ColumnType::Other(_))
}

/// Parse the numeric/temporal **summary** row: columns `mn, mx, distinct_count, null_count` (one
/// row). A missing/short table yields a zeroed summary (no panic).
fn parse_summary(table: &Table) -> FacetResult {
    let cols = table.columns();
    let min = cols.first().and_then(|c| cell_text(c.cells.first()));
    let max = cols.get(1).and_then(|c| cell_text(c.cells.first()));
    let distinct = cols.get(2).map(|c| cell_u64(c.cells.first())).unwrap_or(0);
    let nulls = cols.get(3).map(|c| cell_u64(c.cells.first())).unwrap_or(0);
    FacetResult::Summary {
        min,
        max,
        distinct,
        nulls,
    }
}

/// Parse the text **histogram**: rows of `(value, n, distinct_count, null_count)` where the last
/// two repeat the column-wide counts on every row.
///
/// A **NULL `value` cell is the sentinel row** the `stats LEFT JOIN bars` shape emits when there
/// are zero bars (an all-NULL column): it carries the real distinct/null counts but no bar, so it
/// is skipped from `bars` while its counts are still read. A genuine bar value is never NULL (the
/// query filters `IS NOT NULL`), so this skip never drops a real bar.
fn parse_histogram(table: &Table) -> FacetResult {
    let cols = table.columns();
    let values = cols.first();
    let counts = cols.get(1);
    let bars: Vec<FacetBar> = match (values, counts) {
        (Some(v), Some(n)) => v
            .cells
            .iter()
            .zip(n.cells.iter())
            .filter(|(val, _)| !val.is_null())
            .map(|(val, cnt)| FacetBar::new(val.display(), cell_u64(Some(cnt))))
            .collect(),
        _ => Vec::new(),
    };
    // The column-wide counts repeat on every row (including the sentinel), so the first row always
    // carries them — even when there are no bars.
    let distinct = cols.get(2).map(|c| cell_u64(c.cells.first())).unwrap_or(0);
    let nulls = cols.get(3).map(|c| cell_u64(c.cells.first())).unwrap_or(0);
    FacetResult::Histogram {
        bars,
        distinct,
        nulls,
    }
}

/// A cell's rendered text, or `None` for a NULL / absent cell (an entirely-NULL column makes
/// `min`/`max` return NULL).
fn cell_text(cell: Option<&Cell>) -> Option<String> {
    match cell {
        None | Some(Cell::Null) => None,
        Some(c) => Some(c.display()),
    }
}

/// A cell coerced to `u64` (the count aggregates return `Int`/`Text`); `0` for NULL/absent or an
/// unparseable value (a count is never legitimately negative or non-numeric).
fn cell_u64(cell: Option<&Cell>) -> u64 {
    match cell {
        Some(Cell::Int(i)) => (*i).max(0) as u64,
        Some(Cell::Float(f)) if *f >= 0.0 => *f as u64,
        Some(Cell::Text(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

#[cfg(test)]
#[path = "facet_state_tests.rs"]
mod facet_state_tests;
