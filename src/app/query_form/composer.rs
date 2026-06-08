//! Pure SQL composer for the 5-pane Simple-mode query form.
//!
//! `dev/PLAN.md` post-5 UX redesign (Stage 1 foundation). The Simple-mode query box is five
//! labeled single-line panes — `SELECT` / `WHERE` / `GROUP BY` / `ORDER BY` / `LIMIT` — and the
//! debouncer dispatches the **composed** SQL: `SELECT <projection> FROM t [WHERE …] [GROUP BY …]
//! [ORDER BY …] LIMIT <n>`. This module is the pure projector: trim each pane, emit only the
//! clauses the user filled in, apply the documented LIMIT-pane fallbacks (empty -> default,
//! `0`/`all` -> omit, non-numeric -> error), and return the final SQL string. No engine, no
//! clock, no I/O — just `&str`s in, a `String` out (or a structured error).
//!
//! Pure-core hard floor (`dev/core-modules.txt`): every branch is a real behavior case (which
//! clauses are emitted, which fallbacks fire, which inputs are rejected), and a wrong fallback
//! corrupts the dispatched SQL silently — earns the hard floor.

/// What went wrong composing the composed SQL: today, only a malformed LIMIT pane is rejected
/// (the other panes accept whatever the user typed and let DuckDB / preprocess validate the
/// final SQL). The `reason` is the user-facing message the App shows in the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeError {
    /// The LIMIT pane is non-numeric (and not `all` / `0`), or numeric but out of range. The
    /// `reason` is suitable for display in the status line as-is.
    InvalidLimit { reason: String },
}

impl ComposeError {
    /// The user-facing message — what the App writes to the status line.
    pub fn message(&self) -> &str {
        match self {
            Self::InvalidLimit { reason } => reason,
        }
    }
}

/// Compose the dispatched SQL from the five Simple-mode pane texts.
///
/// Trimming + fallbacks per the locked design (`CLAUDE.md` post-5 UX section):
/// * empty `select` -> emit `*` (composer fallback; the pane text is left empty in the form);
/// * empty `where_clause` / `group_by` / `order_by` -> the clause is omitted entirely;
/// * empty `limit` -> use `default_limit` (composer fallback);
/// * `limit` literal `0` or `all` (case-insensitive, after trim) -> omit the LIMIT clause (no cap);
/// * any other `limit` must parse as a positive `i64` (decimal); otherwise [`ComposeError::InvalidLimit`].
///
/// Identifier quoting is **not** the composer's job — the user types raw SQL into each pane, and
/// the existing preprocess + DuckDB parser validate the final string. This keeps the composer a
/// pure string projector and matches D6's "one place per concern" rule.
pub fn compose_sql(
    select: &str,
    where_clause: &str,
    group_by: &str,
    order_by: &str,
    limit: &str,
    default_limit: usize,
) -> Result<String, ComposeError> {
    let projection = trim_or(select, "*");
    let where_text = where_clause.trim();
    let group_text = group_by.trim();
    let order_text = order_by.trim();
    let limit_clause = compose_limit_clause(limit, default_limit)?;

    let mut sql = String::with_capacity(64);
    sql.push_str("SELECT ");
    sql.push_str(projection);
    sql.push_str(" FROM t");
    if !where_text.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(where_text);
    }
    if !group_text.is_empty() {
        sql.push_str(" GROUP BY ");
        sql.push_str(group_text);
    }
    if !order_text.is_empty() {
        sql.push_str(" ORDER BY ");
        sql.push_str(order_text);
    }
    if let Some(clause) = limit_clause {
        sql.push(' ');
        sql.push_str(&clause);
    }
    Ok(sql)
}

/// The trimmed slice of `s`, or `fallback` when `s` trims to empty. Borrowing returns a `&str` to
/// either argument with the same lifetime story (whichever the caller needs); used for the SELECT
/// pane fallback to `*`.
fn trim_or<'a>(s: &'a str, fallback: &'a str) -> &'a str {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        fallback
    } else {
        trimmed
    }
}

/// Compose the LIMIT clause from the LIMIT pane. Returns `Ok(Some("LIMIT 1000"))` for the common
/// case, `Ok(None)` to omit the clause (`0` / `all`), or [`ComposeError::InvalidLimit`] when the
/// pane has a value that is neither a positive integer nor the documented `all` / `0` keywords.
fn compose_limit_clause(limit: &str, default_limit: usize) -> Result<Option<String>, ComposeError> {
    let trimmed = limit.trim();
    if trimmed.is_empty() {
        return Ok(Some(format!("LIMIT {default_limit}")));
    }
    if trimmed.eq_ignore_ascii_case("all") || trimmed == "0" {
        return Ok(None); // user opted out of the cap
    }
    match trimmed.parse::<i64>() {
        Ok(n) if n > 0 => Ok(Some(format!("LIMIT {n}"))),
        Ok(_) => Err(ComposeError::InvalidLimit {
            reason: invalid_limit_message(),
        }),
        Err(_) => Err(ComposeError::InvalidLimit {
            reason: invalid_limit_message(),
        }),
    }
}

/// The single source-of-truth user-facing message for an invalid LIMIT pane. Kept as a function
/// (not a `const`) so the App and tests pull the same string from the same call site.
pub fn invalid_limit_message() -> String {
    "LIMIT must be a number, 'all', or 0".to_string()
}

#[cfg(test)]
#[path = "composer_tests.rs"]
mod composer_tests;
