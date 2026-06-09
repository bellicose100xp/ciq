//! Pure goldens for the Simple-mode SQL simplifier (Power -> Simple toggle support).

use super::{SimplifyError, try_simplify_from_sql};

#[test]
fn select_star_from_t_simplifies_to_star_projection() {
    let parts = try_simplify_from_sql("SELECT * FROM t LIMIT 1000").unwrap();
    assert_eq!(parts.select, "*");
    assert_eq!(parts.where_clause, "");
    assert_eq!(parts.group_by, "");
    assert_eq!(parts.order_by, "");
    assert_eq!(parts.limit, "1000");
}

#[test]
fn select_with_where_and_order_distributes_into_panes() {
    let parts = try_simplify_from_sql(
        "SELECT id, name FROM t WHERE region = 'EU' ORDER BY id DESC LIMIT 50",
    )
    .unwrap();
    assert_eq!(parts.select, "id, name");
    assert_eq!(parts.where_clause, "region = 'EU'");
    assert_eq!(parts.order_by, "id DESC");
    assert_eq!(parts.limit, "50");
}

#[test]
fn group_by_pane_is_extracted() {
    let parts = try_simplify_from_sql(
        "SELECT region, count(*) FROM t WHERE amount > 0 GROUP BY region ORDER BY count(*) DESC LIMIT 100",
    )
    .unwrap();
    assert_eq!(parts.select, "region, count(*)");
    assert_eq!(parts.where_clause, "amount > 0");
    assert_eq!(parts.group_by, "region");
    assert_eq!(parts.order_by, "count(*) DESC");
    assert_eq!(parts.limit, "100");
}

#[test]
fn no_from_clause_is_accepted() {
    let parts = try_simplify_from_sql("SELECT 1").unwrap();
    assert_eq!(parts.select, "1");
    assert_eq!(parts.limit, "");
}

#[test]
fn distinct_keyword_is_stripped_from_select_pane() {
    let parts = try_simplify_from_sql("SELECT DISTINCT region FROM t").unwrap();
    assert_eq!(parts.select, "region");
}

#[test]
fn case_insensitive_table_name_is_accepted() {
    let parts = try_simplify_from_sql("select * from T").unwrap();
    assert_eq!(parts.select, "*");
}

#[test]
fn trailing_semicolon_is_tolerated() {
    let parts = try_simplify_from_sql("SELECT * FROM t LIMIT 10;").unwrap();
    assert_eq!(parts.select, "*");
    assert_eq!(parts.limit, "10");
}

// --- rejections ---

#[test]
fn join_is_rejected() {
    let err = try_simplify_from_sql("SELECT * FROM t JOIN u ON t.id = u.id").unwrap_err();
    assert!(matches!(err, SimplifyError::ContainsJoin));
}

#[test]
fn cte_is_rejected() {
    let err = try_simplify_from_sql("WITH x AS (SELECT 1) SELECT * FROM x").unwrap_err();
    assert!(matches!(err, SimplifyError::ContainsCte));
}

#[test]
fn subquery_in_select_is_rejected() {
    let err = try_simplify_from_sql("SELECT (SELECT count(*) FROM t) FROM t").unwrap_err();
    assert!(matches!(err, SimplifyError::ContainsSubquery));
}

#[test]
fn from_subquery_is_rejected() {
    let err = try_simplify_from_sql("SELECT * FROM (SELECT * FROM t)").unwrap_err();
    assert!(matches!(err, SimplifyError::ContainsSubquery));
}

#[test]
fn having_is_rejected() {
    let err =
        try_simplify_from_sql("SELECT region, count(*) FROM t GROUP BY region HAVING count(*) > 1")
            .unwrap_err();
    assert!(matches!(err, SimplifyError::ContainsHaving));
}

#[test]
fn from_other_table_is_rejected() {
    let err = try_simplify_from_sql("SELECT * FROM other").unwrap_err();
    assert!(matches!(err, SimplifyError::NonTTable));
}

#[test]
fn multi_statement_is_rejected() {
    let err = try_simplify_from_sql("SELECT 1; SELECT 2").unwrap_err();
    assert!(matches!(err, SimplifyError::MultiStatement));
}

#[test]
fn dml_is_rejected_as_not_a_select() {
    let err = try_simplify_from_sql("DELETE FROM t").unwrap_err();
    assert!(matches!(err, SimplifyError::NotASelect));
}

#[test]
fn empty_sql_is_rejected() {
    let err = try_simplify_from_sql("   ").unwrap_err();
    assert!(matches!(err, SimplifyError::NotASelect));
}

#[test]
fn error_messages_are_user_facing() {
    let err = try_simplify_from_sql("SELECT * FROM t JOIN u ON x").unwrap_err();
    assert_eq!(err.message(), "contains a JOIN");
    let err = try_simplify_from_sql("WITH x AS (SELECT 1) SELECT * FROM x").unwrap_err();
    assert_eq!(err.message(), "contains a CTE / WITH clause");
}

#[test]
fn out_of_order_where_after_order_does_not_panic_returns_other_error() {
    // Regression: pre-fix this slice-panicked because `body_start = tokens[where].end` (later)
    // was greater than `body_end = tokens[order].start` (earlier).
    let err = try_simplify_from_sql("SELECT * FROM t ORDER BY x WHERE y").unwrap_err();
    match err {
        SimplifyError::Other(msg) => assert_eq!(msg, "out-of-order clauses"),
        other => panic!("expected Other('out-of-order clauses'), got {other:?}"),
    }
}

#[test]
fn out_of_order_limit_before_order_does_not_panic_returns_other_error() {
    let err = try_simplify_from_sql("SELECT * FROM t LIMIT 10 ORDER BY x").unwrap_err();
    assert!(matches!(err, SimplifyError::Other(_)));
}

#[test]
fn out_of_order_group_after_order_does_not_panic_returns_other_error() {
    let err = try_simplify_from_sql("SELECT * FROM t ORDER BY x GROUP BY y").unwrap_err();
    assert!(matches!(err, SimplifyError::Other(_)));
}

#[test]
fn left_and_right_function_calls_are_accepted_not_join() {
    // Regression: pre-fix the bare keywords `left` / `right` (DuckDB string functions) were
    // wrongly rejected as joins. Valid `SELECT left(name, 3), right(name, 3) FROM t` round-trips.
    let parts =
        try_simplify_from_sql("SELECT left(name, 3), right(name, 3) FROM t LIMIT 100").unwrap();
    assert_eq!(parts.select, "left(name, 3), right(name, 3)");
    assert_eq!(parts.limit, "100");
}

#[test]
fn distinct_followed_by_tab_strips_the_keyword() {
    // Regression: the strip predicate previously matched only an ASCII space after `distinct`,
    // so a tab/newline left the SELECT pane as `"DISTINCT\tregion"`. Any ASCII whitespace works now.
    let parts = try_simplify_from_sql("SELECT DISTINCT\tregion FROM t").unwrap();
    assert_eq!(parts.select, "region");
    let parts = try_simplify_from_sql("SELECT DISTINCT\nregion FROM t").unwrap();
    assert_eq!(parts.select, "region");
}
