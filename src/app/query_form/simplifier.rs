//! Pure SQL simplifier — parse a single restricted `SELECT … FROM t …` into the 5-pane shape.
//!
//! `dev/PLAN.md` post-5 UX redesign (Stage 1 foundation). Companion to [`composer`](super::composer):
//! the composer goes pane texts -> SQL string; the simplifier goes SQL string -> pane texts. Used
//! when the user toggles **Power -> Simple** (`Ctrl+Q`): if the Power textarea contains a clean
//! single-`SELECT` against `t`, distribute its clauses into the five Simple panes; otherwise
//! reject with a structured reason and stay in Power mode.
//!
//! Reuses the shared [`crate::sql_lexer`] (D6 — one place per concern; never spin up a parallel
//! scanner). Reads the token stream, checks the rejection invariants at top-level paren depth,
//! finds clause keyword spans, and slices the **original source** between them so the panes
//! preserve the user's casing and whitespace exactly.
//!
//! Pure-core hard floor (`dev/core-modules.txt`): every branch is a real behavior case (each
//! rejection reason, each clause split), and a wrong split silently corrupts the round-trip
//! through the form — earns the hard floor.

use crate::sql_lexer::{Token, TokenKind, tokenize};

/// The five pane texts the simplifier produces from a clean SELECT. Whitespace is trimmed; the
/// user's casing inside each clause is preserved verbatim. The LIMIT field carries the literal
/// pane text (`"1000"` for an explicit limit, `"1000"` for none — the App seeds the default
/// before calling). The simplifier itself never substitutes a default — it only fills `limit`
/// when the source had one. The App caller fills the default for the empty case.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SimpleParts {
    pub select: String,
    pub where_clause: String,
    pub group_by: String,
    pub order_by: String,
    pub limit: String,
}

/// Why a SQL string can't be simplified into the 5-pane shape. Each variant maps to the user-
/// facing status-line reason the App shows when toggling refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimplifyError {
    /// Multiple top-level statements (a top-level `;` other than the trailing one).
    MultiStatement,
    /// The query isn't a `SELECT` (DML/DDL, no leading SELECT, or empty).
    NotASelect,
    /// A `WITH` / CTE preface — Simple mode only models a bare SELECT.
    ContainsCte,
    /// `JOIN` (INNER/LEFT/…) — Simple mode is single-table.
    ContainsJoin,
    /// A subquery in the FROM or projection list — Simple mode doesn't model nesting.
    ContainsSubquery,
    /// `HAVING` — Simple mode doesn't expose a HAVING pane.
    ContainsHaving,
    /// The `FROM` target is not the single identifier `t`.
    NonTTable,
    /// A structural surprise the simplifier didn't expect (mis-ordered clauses, etc.).
    Other(String),
}

impl SimplifyError {
    /// The user-facing reason ("can't simplify: <reason>") — what the App writes to the status
    /// line when Power -> Simple toggle refuses.
    pub fn message(&self) -> String {
        match self {
            Self::MultiStatement => "multiple statements".to_string(),
            Self::NotASelect => "not a SELECT statement".to_string(),
            Self::ContainsCte => "contains a CTE / WITH clause".to_string(),
            Self::ContainsJoin => "contains a JOIN".to_string(),
            Self::ContainsSubquery => "contains a subquery".to_string(),
            Self::ContainsHaving => "contains a HAVING clause".to_string(),
            Self::NonTTable => "FROM target is not `t`".to_string(),
            Self::Other(msg) => msg.clone(),
        }
    }
}

/// Try to simplify `sql` into the 5-pane shape; on success, return the per-pane substrings.
///
/// The accepted shape is the bare `SELECT [DISTINCT] <projection> [FROM t] [WHERE …] [GROUP BY …]
/// [ORDER BY …] [LIMIT …]` form (a single statement, no joins/CTEs/subqueries/having). The FROM
/// is **optional** (`SELECT 1` is fine) but if present its table must be `t` (any case). When
/// `SELECT *` is parsed, the SELECT pane comes back as `"*"` so a round-trip preserves the form.
pub fn try_simplify_from_sql(sql: &str) -> Result<SimpleParts, SimplifyError> {
    let tokens = tokenize(sql);
    let content: Vec<usize> = tokens
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.is_trivia())
        .map(|(i, _)| i)
        .collect();

    if content.is_empty() {
        return Err(SimplifyError::NotASelect);
    }

    // Top-level `;` other than at the very end (or a single trailing one) means multi-statement.
    let semicolons: Vec<usize> = content
        .iter()
        .copied()
        .filter(|&i| {
            let t = tokens[i];
            t.kind == TokenKind::Punct && t.depth == 0 && token_text(sql, t) == ";"
        })
        .collect();
    let last_idx = *content.last().unwrap();
    if semicolons.iter().any(|&i| i != last_idx) {
        return Err(SimplifyError::MultiStatement);
    }
    let trailing_semicolon = semicolons.last().copied();
    // Working content list excludes any trailing `;`.
    let content_no_semi: Vec<usize> = content
        .iter()
        .copied()
        .filter(|i| Some(*i) != trailing_semicolon)
        .collect();
    if content_no_semi.is_empty() {
        return Err(SimplifyError::NotASelect);
    }

    let first_tok = tokens[content_no_semi[0]];
    let first_text = token_text(sql, first_tok);
    if first_tok.kind == TokenKind::Keyword && first_text.eq_ignore_ascii_case("with") {
        return Err(SimplifyError::ContainsCte);
    }
    if !(first_tok.kind == TokenKind::Keyword && first_text.eq_ignore_ascii_case("select")) {
        return Err(SimplifyError::NotASelect);
    }

    // Reject the disallowed top-level keywords anywhere in the statement.
    for &idx in &content_no_semi {
        let t = tokens[idx];
        if t.depth != 0 || t.kind != TokenKind::Keyword {
            continue;
        }
        let text = token_text(sql, t);
        if text.eq_ignore_ascii_case("join")
            || text.eq_ignore_ascii_case("inner")
            || text.eq_ignore_ascii_case("left")
            || text.eq_ignore_ascii_case("right")
            || text.eq_ignore_ascii_case("full")
            || text.eq_ignore_ascii_case("outer")
            || text.eq_ignore_ascii_case("cross")
        {
            return Err(SimplifyError::ContainsJoin);
        }
        if text.eq_ignore_ascii_case("having") {
            return Err(SimplifyError::ContainsHaving);
        }
        if text.eq_ignore_ascii_case("union")
            || text.eq_ignore_ascii_case("except")
            || text.eq_ignore_ascii_case("intersect")
        {
            return Err(SimplifyError::Other("set operation".to_string()));
        }
    }

    // Identify the top-level clause keyword positions (in order they appear). FROM may be
    // followed by a `(` — that's a FROM-subquery, which we reject upfront.
    let select_idx = content_no_semi[0];
    let from_idx = find_top_level_keyword(&content_no_semi, &tokens, sql, "from");
    let where_idx = find_top_level_keyword(&content_no_semi, &tokens, sql, "where");
    let group_idx = find_top_level_pair(&content_no_semi, &tokens, sql, "group", "by");
    let order_idx = find_top_level_pair(&content_no_semi, &tokens, sql, "order", "by");
    let limit_idx = find_top_level_keyword(&content_no_semi, &tokens, sql, "limit");

    if let Some(fi) = from_idx {
        // Validate the FROM target: the next content token must be the bare identifier `t`.
        let after_from = next_content_after(&content_no_semi, fi);
        let target_idx = after_from.ok_or(SimplifyError::NonTTable)?;
        let target = tokens[target_idx];
        let text = token_text(sql, target);
        if target.kind == TokenKind::Punct && text == "(" {
            return Err(SimplifyError::ContainsSubquery);
        }
        if target.kind != TokenKind::Ident || !text.eq_ignore_ascii_case("t") {
            return Err(SimplifyError::NonTTable);
        }
        // The FROM's tail must be just `t` — the next content token (if any) must be a known
        // clause keyword (WHERE/GROUP/ORDER/LIMIT). A trailing alias / a comma / another ident is
        // a non-T shape we don't model.
        if let Some(next_idx) = next_content_after(&content_no_semi, target_idx) {
            let next = tokens[next_idx];
            let next_text = token_text(sql, next);
            let is_clause_kw = next.kind == TokenKind::Keyword
                && (next_text.eq_ignore_ascii_case("where")
                    || next_text.eq_ignore_ascii_case("group")
                    || next_text.eq_ignore_ascii_case("order")
                    || next_text.eq_ignore_ascii_case("limit"));
            if !is_clause_kw {
                return Err(SimplifyError::NonTTable);
            }
        }
    }

    // Detect a top-level subquery / parenthesized SELECT in the projection or WHERE clauses by
    // scanning the body for a `(` that opens a sub-SELECT. We lazily approximate: a `(` followed
    // by a top-of-paren-stack `select` keyword is a subquery. Top-level parens (e.g. `coalesce(`,
    // a tuple value `(1, 2)`) are fine.
    if has_inner_select(&tokens, sql) {
        return Err(SimplifyError::ContainsSubquery);
    }

    // Slice the original source for each clause body. The body for a clause `K` runs from just
    // past `K`'s last keyword token up to the start of the next clause keyword (or the end of
    // the statement, sans the trailing `;` if any).
    let stmt_end = trailing_semicolon
        .map(|i| tokens[i].start)
        .unwrap_or_else(|| sql.len());

    let select_body_start = tokens[select_idx].end;
    let select_body_end = first_index_pos(
        sql, &tokens, from_idx, where_idx, group_idx, order_idx, limit_idx,
    )
    .unwrap_or(stmt_end);
    let mut select_body = sql[select_body_start..select_body_end].trim().to_string();
    // Optional leading DISTINCT: keep it. `SELECT *` -> projection text "*"; preserve as-is.
    if select_body.is_empty() {
        return Err(SimplifyError::NotASelect);
    }

    let where_body = clause_body(
        sql,
        &tokens,
        where_idx,
        &[group_idx, order_idx, limit_idx],
        stmt_end,
    );
    let group_body = if let Some(gi) = group_idx {
        // group_idx points at the `group` keyword; the body starts after the following `by`.
        let body_start = after_by(&tokens, gi);
        let body_end =
            first_index_pos_among(sql, &tokens, &[order_idx, limit_idx]).unwrap_or(stmt_end);
        sql[body_start..body_end].trim().to_string()
    } else {
        String::new()
    };
    let order_body = if let Some(oi) = order_idx {
        let body_start = after_by(&tokens, oi);
        let body_end = first_index_pos_among(sql, &tokens, &[limit_idx]).unwrap_or(stmt_end);
        sql[body_start..body_end].trim().to_string()
    } else {
        String::new()
    };
    let limit_body = if let Some(li) = limit_idx {
        sql[tokens[li].end..stmt_end].trim().to_string()
    } else {
        String::new()
    };

    // Strip a leading `DISTINCT` from the SELECT projection so the SELECT pane round-trips
    // sanely; the user's projection is what they care about. (Stage 1 doesn't expose a DISTINCT
    // checkbox; preserving DISTINCT in the pane text would round-trip into "SELECT DISTINCT
    // DISTINCT" once the composer prepends nothing.)
    let lower = select_body.to_ascii_lowercase();
    if lower.starts_with("distinct ") || lower == "distinct" {
        // Re-emit the original casing's leading keyword stripped: the original first 8 chars are
        // `distinct` in some case; drop them + any single following whitespace char.
        let rest = &select_body[8..];
        select_body = rest.trim_start().to_string();
        if select_body.is_empty() {
            return Err(SimplifyError::NotASelect);
        }
    }

    Ok(SimpleParts {
        select: select_body,
        where_clause: where_body,
        group_by: group_body,
        order_by: order_body,
        limit: limit_body,
    })
}

/// The slice of `src` for a token's span — kept short at the call sites.
fn token_text(src: &str, t: Token) -> &str {
    &src[t.start..t.end]
}

/// Find the index (into `tokens`) of the first top-level (depth 0) `Keyword` whose text matches
/// `kw` (case-insensitive). Restricted to the `content_no_semi` indexes so trivia/`;` are skipped.
fn find_top_level_keyword(
    content: &[usize],
    tokens: &[Token],
    sql: &str,
    kw: &str,
) -> Option<usize> {
    for &idx in content {
        let t = tokens[idx];
        if t.kind == TokenKind::Keyword
            && t.depth == 0
            && token_text(sql, t).eq_ignore_ascii_case(kw)
        {
            return Some(idx);
        }
    }
    None
}

/// Find the index of a top-level keyword pair like `GROUP BY` / `ORDER BY` — the index of the
/// first keyword (`group` / `order`) at depth 0 immediately followed by a `by` keyword (with
/// only trivia between them). Returns the index of the first keyword.
fn find_top_level_pair(
    content: &[usize],
    tokens: &[Token],
    sql: &str,
    kw1: &str,
    kw2: &str,
) -> Option<usize> {
    let mut iter = content.iter().copied().peekable();
    while let Some(idx) = iter.next() {
        let t = tokens[idx];
        if t.kind != TokenKind::Keyword
            || t.depth != 0
            || !token_text(sql, t).eq_ignore_ascii_case(kw1)
        {
            continue;
        }
        // The next content token must be the matching second keyword.
        if let Some(&next_idx) = iter.peek() {
            let n = tokens[next_idx];
            if n.kind == TokenKind::Keyword
                && n.depth == 0
                && token_text(sql, n).eq_ignore_ascii_case(kw2)
            {
                return Some(idx);
            }
        }
    }
    None
}

/// The next content (non-trivia) token index strictly after `from_idx`.
fn next_content_after(content: &[usize], from_idx: usize) -> Option<usize> {
    content.iter().copied().find(|&i| i > from_idx)
}

/// Return whether the token stream contains a parenthesized inner SELECT — i.e. an `(` token
/// (depth opens to 1+) followed by a `select` keyword inside that paren group. Top-level
/// function calls (`coalesce(`) don't qualify because no SELECT keyword sits inside their
/// content tokens at higher depth.
fn has_inner_select(tokens: &[Token], sql: &str) -> bool {
    for t in tokens {
        if t.kind == TokenKind::Keyword
            && t.depth > 0
            && token_text(sql, *t).eq_ignore_ascii_case("select")
        {
            return true;
        }
    }
    false
}

/// The byte offset where the first of the supplied clause keyword indexes (if any) begins.
/// Returns `None` when none of the indexes are `Some`.
fn first_index_pos(
    sql: &str,
    tokens: &[Token],
    from_idx: Option<usize>,
    where_idx: Option<usize>,
    group_idx: Option<usize>,
    order_idx: Option<usize>,
    limit_idx: Option<usize>,
) -> Option<usize> {
    let _ = sql;
    [from_idx, where_idx, group_idx, order_idx, limit_idx]
        .into_iter()
        .flatten()
        .map(|i| tokens[i].start)
        .min()
}

/// The byte offset where the first of the supplied clause keyword indexes begins.
fn first_index_pos_among(sql: &str, tokens: &[Token], idxs: &[Option<usize>]) -> Option<usize> {
    let _ = sql;
    idxs.iter()
        .copied()
        .flatten()
        .map(|i| tokens[i].start)
        .min()
}

/// Slice the body for a single-keyword clause (WHERE / LIMIT) — from just past the keyword to the
/// next clause's start (whichever of `next_idxs` comes first), or `stmt_end` when none follow.
fn clause_body(
    sql: &str,
    tokens: &[Token],
    clause_idx: Option<usize>,
    next_idxs: &[Option<usize>],
    stmt_end: usize,
) -> String {
    let Some(ci) = clause_idx else {
        return String::new();
    };
    let body_start = tokens[ci].end;
    let body_end = first_index_pos_among(sql, tokens, next_idxs).unwrap_or(stmt_end);
    sql[body_start..body_end].trim().to_string()
}

/// The byte offset just past the `BY` keyword that follows `GROUP` / `ORDER`. The `find_top_level_pair`
/// guarantees a content `by` follows immediately, so we skip exactly the next non-trivia token.
fn after_by(tokens: &[Token], pair_idx: usize) -> usize {
    let mut i = pair_idx + 1;
    while i < tokens.len() && tokens[i].is_trivia() {
        i += 1;
    }
    if i < tokens.len() {
        tokens[i].end
    } else {
        // Defensive: no BY found — shouldn't happen because find_top_level_pair already verified it.
        tokens[pair_idx].end
    }
}

#[cfg(test)]
#[path = "simplifier_tests.rs"]
mod simplifier_tests;
