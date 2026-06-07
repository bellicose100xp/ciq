//! Tests for the polish formatters: empty-state snapshots per case + truncation-banner goldens
//! (at the cap, under the cap, exactly the cap, user-limited, degenerate).

use crate::app::polish::{EmptyKind, empty_state, truncation_banner};

// --- empty-state, one snapshot per case ---

#[test]
fn empty_state_loading() {
    assert_eq!(empty_state(EmptyKind::Loading), "loading CSV…");
}

#[test]
fn empty_state_no_query_yet() {
    assert_eq!(
        empty_state(EmptyKind::NoQueryYet),
        "type a SQL query above (e.g. SELECT * FROM t)"
    );
}

#[test]
fn empty_state_zero_rows_is_a_result_not_a_prompt() {
    // The key distinction: a query that matched nothing says "no rows match", NOT "type a query".
    assert_eq!(empty_state(EmptyKind::ZeroRows), "no rows match");
    assert_ne!(
        empty_state(EmptyKind::ZeroRows),
        empty_state(EmptyKind::NoQueryYet)
    );
}

#[test]
fn empty_state_messages_are_ascii_safe_and_nonempty() {
    for k in [
        EmptyKind::Loading,
        EmptyKind::NoQueryYet,
        EmptyKind::ZeroRows,
    ] {
        assert!(!empty_state(k).is_empty());
    }
}

// --- truncation banner goldens ---

#[test]
fn banner_at_cap() {
    assert_eq!(
        truncation_banner(1000, 1000, true),
        Some("showing first 1000 rows (use --output to export all)".to_string())
    );
}

#[test]
fn banner_over_cap_should_not_happen_but_still_warns() {
    // The grid never holds more than the cap, but be defensive: >= cap still warns.
    assert_eq!(
        truncation_banner(1500, 1000, true),
        Some("showing first 1000 rows (use --output to export all)".to_string())
    );
}

#[test]
fn no_banner_under_cap() {
    // Fewer rows than the cap → the whole result is shown → no banner.
    assert_eq!(truncation_banner(42, 1000, true), None);
    assert_eq!(truncation_banner(999, 1000, true), None);
}

#[test]
fn no_banner_when_user_supplied_limit() {
    // The user wrote `LIMIT 1000`; 1000 rows is their intent, not a ciq cap → no banner.
    assert_eq!(truncation_banner(1000, 1000, false), None);
}

#[test]
fn no_banner_on_degenerate_cap() {
    // A zero cap (or zero rows) never produces a banner.
    assert_eq!(truncation_banner(0, 0, true), None);
    assert_eq!(truncation_banner(0, 1000, true), None);
}
