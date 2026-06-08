//! Pure goldens for the Simple-mode SQL composer.

use super::{ComposeError, compose_sql, invalid_limit_message};

/// Helper: shorter call signature in the table.
fn compose(
    select: &str,
    w: &str,
    g: &str,
    o: &str,
    l: &str,
    default_limit: usize,
) -> Result<String, ComposeError> {
    compose_sql(select, w, g, o, l, default_limit)
}

#[test]
fn defaults_emit_select_star_with_default_limit() {
    let sql = compose("", "", "", "", "", 1000).unwrap();
    assert_eq!(sql, "SELECT * FROM t LIMIT 1000");
}

#[test]
fn explicit_star_in_select_pane_is_preserved() {
    let sql = compose("*", "", "", "", "1000", 1000).unwrap();
    assert_eq!(sql, "SELECT * FROM t LIMIT 1000");
}

#[test]
fn projection_list_in_select_pane_appears_verbatim() {
    let sql = compose("id, name", "", "", "", "1000", 1000).unwrap();
    assert_eq!(sql, "SELECT id, name FROM t LIMIT 1000");
}

#[test]
fn where_pane_emits_where_clause() {
    let sql = compose("*", "region = 'EU'", "", "", "1000", 1000).unwrap();
    assert_eq!(sql, "SELECT * FROM t WHERE region = 'EU' LIMIT 1000");
}

#[test]
fn empty_where_pane_omits_where_clause() {
    let sql = compose("id", "", "", "", "1000", 1000).unwrap();
    assert_eq!(sql, "SELECT id FROM t LIMIT 1000");
}

#[test]
fn group_by_pane_emits_clause() {
    let sql = compose("region, count(*)", "", "region", "", "1000", 1000).unwrap();
    assert_eq!(
        sql,
        "SELECT region, count(*) FROM t GROUP BY region LIMIT 1000"
    );
}

#[test]
fn order_by_pane_emits_clause() {
    let sql = compose("id", "", "", "id desc", "1000", 1000).unwrap();
    assert_eq!(sql, "SELECT id FROM t ORDER BY id desc LIMIT 1000");
}

#[test]
fn all_clauses_compose_in_canonical_order() {
    let sql = compose(
        "region, count(*)",
        "amount > 0",
        "region",
        "count(*) desc",
        "100",
        1000,
    )
    .unwrap();
    assert_eq!(
        sql,
        "SELECT region, count(*) FROM t WHERE amount > 0 GROUP BY region \
         ORDER BY count(*) desc LIMIT 100"
    );
}

#[test]
fn limit_pane_overrides_the_default() {
    let sql = compose("*", "", "", "", "42", 1000).unwrap();
    assert_eq!(sql, "SELECT * FROM t LIMIT 42");
}

#[test]
fn limit_pane_empty_falls_back_to_the_default() {
    let sql = compose("*", "", "", "", "", 500).unwrap();
    assert_eq!(sql, "SELECT * FROM t LIMIT 500");
}

#[test]
fn limit_pane_zero_omits_the_limit_clause() {
    let sql = compose("*", "", "", "", "0", 1000).unwrap();
    assert_eq!(sql, "SELECT * FROM t");
}

#[test]
fn limit_pane_all_case_insensitive_omits_the_limit_clause() {
    let sql = compose("*", "", "", "", "all", 1000).unwrap();
    assert_eq!(sql, "SELECT * FROM t");
    let sql = compose("*", "", "", "", "ALL", 1000).unwrap();
    assert_eq!(sql, "SELECT * FROM t");
    let sql = compose("*", "", "", "", "  All  ", 1000).unwrap();
    assert_eq!(sql, "SELECT * FROM t");
}

#[test]
fn limit_pane_non_numeric_is_an_error_with_status_message() {
    let err = compose("*", "", "", "", "abc", 1000).unwrap_err();
    let ComposeError::InvalidLimit { reason } = err;
    assert_eq!(reason, invalid_limit_message());
}

#[test]
fn limit_pane_negative_is_an_error() {
    let err = compose("*", "", "", "", "-5", 1000).unwrap_err();
    let ComposeError::InvalidLimit { .. } = err;
}

#[test]
fn limit_pane_overflow_is_an_error() {
    let err = compose("*", "", "", "", "999999999999999999999", 1000).unwrap_err();
    let ComposeError::InvalidLimit { .. } = err;
}

#[test]
fn whitespace_in_panes_is_trimmed() {
    let sql = compose("  id  ", "  amount > 0  ", "", "  id  ", "  100  ", 1000).unwrap();
    assert_eq!(
        sql,
        "SELECT id FROM t WHERE amount > 0 ORDER BY id LIMIT 100"
    );
}

#[test]
fn select_pane_only_whitespace_falls_back_to_star() {
    let sql = compose("   ", "", "", "", "1000", 1000).unwrap();
    assert_eq!(sql, "SELECT * FROM t LIMIT 1000");
}
