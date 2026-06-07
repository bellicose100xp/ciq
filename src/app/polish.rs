//! Polish — pure formatting for the empty-state message and the large-result truncation banner.
//!
//! `dev/PLAN.md` §7 P5.3 + §6.4. Two small pure helpers, both `data -> String`, snapshot/golden
//! tested:
//!  - [`empty_state`] — the message shown in the results pane when there is nothing to render: a
//!    "type a query" hint before the first query, a "no rows match" notice when a query ran but
//!    matched zero rows, and a "loading" line while the CSV is still parsing. The distinction
//!    matters: an empty grid after a `WHERE` that filtered everything out is a *result*, not a
//!    prompt to start typing.
//!  - [`truncation_banner`] — the "showing first N rows (use --output for all)" line shown when the
//!    interactive viewport `LIMIT N` capped the grid. Derived from the displayed row count, the cap,
//!    and whether ciq applied the wrap (the user's own `LIMIT` is their intent, not a ciq cap) — so
//!    **no extra `COUNT(*)` query** is needed: hitting the cap is itself the signal that more rows
//!    may exist.
//!
//! Pure: no `Frame`, no clock, no engine. The App calls these with plain data and the render layer
//! paints the returned strings via [`theme`](crate::theme).

/// Which empty-results situation the pane is in (drives [`empty_state`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyKind {
    /// The CSV is still being parsed; no engine yet. ("loading…")
    Loading,
    /// Loaded and idle, but the user hasn't run a query yet. ("type a query…")
    NoQueryYet,
    /// A query ran and returned zero rows — a genuine empty *result*, not a prompt.
    ZeroRows,
}

/// The empty-state message for `kind`. Pure; the render layer styles it (loading dimmed, the
/// zero-rows notice as normal status, the hint as a quiet prompt).
pub fn empty_state(kind: EmptyKind) -> &'static str {
    match kind {
        EmptyKind::Loading => "loading CSV…",
        EmptyKind::NoQueryYet => "type a SQL query above (e.g. SELECT * FROM t)",
        EmptyKind::ZeroRows => "no rows match",
    }
}

/// The truncation banner, or `None` when the grid is not capped by ciq.
///
/// A banner is warranted only when **ciq** capped the result: `ciq_capped` is true (the user
/// supplied no `LIMIT`, so [`prepare_interactive`](crate::query::preprocess::prepare_interactive)
/// wrapped the query in `… LIMIT cap`) AND the displayed row count reached the cap. When the user
/// wrote their own `LIMIT`, the row count is their intent — no banner. When fewer rows came back
/// than the cap, the whole result is shown — no banner.
///
/// Hitting the cap is the signal that more rows *may* exist; ciq does not run a second `COUNT(*)`
/// to confirm, so the wording is "showing first N rows" (a statement about what's displayed), not
/// "N of M" (which would need the true total).
pub fn truncation_banner(displayed_rows: usize, cap: usize, ciq_capped: bool) -> Option<String> {
    if ciq_capped && cap > 0 && displayed_rows >= cap {
        Some(format!(
            "showing first {cap} rows (use --output to export all)"
        ))
    } else {
        None
    }
}

#[cfg(test)]
#[path = "polish_tests.rs"]
mod polish_tests;
