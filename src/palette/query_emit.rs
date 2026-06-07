//! Query emitter — pure `emit(&PaletteState) -> String` (`dev/PLAN.md` §6.2, `dev/DECISIONS.md`
//! D3).
//!
//! Renders the palette's structured state into a canonical
//! `SELECT <projection> FROM t [WHERE <conjunction>] [ORDER BY …] LIMIT <k>`. **Pure `state ->
//! String`** — no parser, no `Schema` lookup (the predicates carry their column type), no engine,
//! no clock. Named `query_emit` (not `emit.rs`) to avoid colliding with `output/emit.rs` (§0/D3).
//!
//! The emitted byte format is a **stable identity surface**: the ownership check (P4.4) byte-
//! compares the bar text against the last string this produced, so its formatting is fixed, not a
//! free internal choice. It is therefore exhaustively golden-tested, including the **two** quoting
//! surfaces D3 calls out:
//!  - (a) **identifier quoting in the projection** — a column named `order` or `Total ($)` is
//!    double-quoted (`"order"`, `"Total ($)"`) via the shared
//!    [`quote_ident_if_needed`](crate::sql_ident::quote_ident_if_needed);
//!  - (b) **facet-predicate value quoting/escaping** — `region = 'O''Brien'` (embedded quote
//!    doubled), `col IS NULL` (NULL handling), numeric `5` bare vs string `'5'`, dates as string
//!    literals — via the shared [`single_quote_literal`](crate::sql_ident::single_quote_literal).
//!
//! **Reorder is its own exit criterion (§0/D3):** the checked-column selection order drives the
//! projection order, so reordering the selection reorders the emitted `SELECT` list.

use crate::engine::duckdb_engine::TABLE;
use crate::sql_ident::{quote_ident_if_needed, render_typed_value, single_quote_literal};

use super::palette_state::{PaletteState, Predicate, PredicateOp};

/// The default display-row cap for an emitted palette query — the same screenful-plus-margin
/// budget the interactive LIMIT-wrap uses ([`crate::app::VIEWPORT_ROW_LIMIT`]). DuckDB returns
/// `min(k, N)` rows when it applies `LIMIT k`, which is the §2.3 / D3 display-limit rule.
pub const DEFAULT_LIMIT: usize = crate::app::VIEWPORT_ROW_LIMIT;

/// Emit the canonical query for `state` at the default display limit ([`DEFAULT_LIMIT`]).
pub fn emit(state: &PaletteState) -> String {
    emit_with_limit(state, DEFAULT_LIMIT)
}

/// Emit the canonical query for `state`, capping the result at `limit` rows.
///
/// Shape: `SELECT <projection> FROM t [WHERE <p1 AND p2 …>] LIMIT <limit>`. The projection is the
/// checked columns in selection order, each identifier-quoted only when it needs it; an **empty**
/// selection emits `SELECT *` (the all-columns wildcard). Predicates form an `AND` conjunction with
/// type-correct value quoting. There is no `ORDER BY` in v1 (the palette has no sort affordance
/// yet; reordering changes the *projection*, not the sort) — the clause is reserved for a later
/// pass, hence the `[ORDER BY …]` in the doc shape.
pub fn emit_with_limit(state: &PaletteState, limit: usize) -> String {
    let mut out = String::from("SELECT ");
    out.push_str(&projection(state));
    out.push_str(" FROM ");
    out.push_str(TABLE);

    if !state.predicates().is_empty() {
        out.push_str(" WHERE ");
        out.push_str(&conjunction(state.predicates()));
    }

    out.push_str(" LIMIT ");
    out.push_str(&limit.to_string());
    out
}

/// The projection clause: the checked columns in selection order (identifier-quoted as needed),
/// comma-separated; or `*` when nothing is checked.
fn projection(state: &PaletteState) -> String {
    let checked = state.checked_columns();
    if checked.is_empty() {
        return "*".to_string();
    }
    checked
        .iter()
        .map(|c| quote_ident_if_needed(&c.name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The `WHERE` conjunction: each predicate rendered and joined with ` AND `.
fn conjunction(predicates: &[Predicate]) -> String {
    predicates
        .iter()
        .map(render_predicate)
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Render one predicate as SQL with type-correct value quoting.
///
/// A NULL test becomes `col IS NULL` (for `Eq`) / `col IS NOT NULL` (for `Neq`); any other op with
/// no value also falls back to the IS [NOT] NULL form for the comparison ops (a value-less `<`/`>`
/// is meaningless, so it is treated as a presence test) — but the public API only constructs
/// value-less predicates via `Predicate::null_test`, which the App restricts to `Eq`/`Neq`.
/// A value predicate renders `col <op> <quoted-value>`, the value bare for numeric/bool columns and
/// single-quoted for text/temporal (and unknown) columns — **except** a `LIKE`/`NOT LIKE` pattern,
/// which is always a string literal regardless of the column's type (so `code LIKE 5` on a numeric
/// column emits `code LIKE '5'`, the intended string match, not a bare numeric).
fn render_predicate(p: &Predicate) -> String {
    let col = quote_ident_if_needed(&p.column);
    match &p.value {
        None => {
            let neg = matches!(p.op, PredicateOp::Neq);
            if neg {
                format!("{col} IS NOT NULL")
            } else {
                format!("{col} IS NULL")
            }
        }
        Some(value) => {
            let op = op_sql(p.op);
            let rendered = if matches!(p.op, PredicateOp::Like) {
                // A LIKE pattern is always a string literal, independent of the column type.
                single_quote_literal(value)
            } else {
                render_typed_value(value, Some(&p.ty))
            };
            format!("{col} {op} {rendered}")
        }
    }
}

/// The SQL text for a value-predicate operator.
fn op_sql(op: PredicateOp) -> &'static str {
    match op {
        PredicateOp::Eq => "=",
        PredicateOp::Neq => "!=",
        PredicateOp::Lt => "<",
        PredicateOp::Le => "<=",
        PredicateOp::Gt => ">",
        PredicateOp::Ge => ">=",
        PredicateOp::Like => "LIKE",
    }
}

#[cfg(test)]
#[path = "query_emit_tests.rs"]
mod query_emit_tests;
