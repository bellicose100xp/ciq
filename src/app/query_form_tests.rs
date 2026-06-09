//! Headless tests for the [`QueryForm`] container — focus cycling, composer wiring, mode
//! toggle (Simple <-> Power), and the LIMIT-pane error machinery.

use super::{QueryForm, QueryMode, SimplePane};

#[test]
fn defaults_seed_select_star_and_limit_1000_focused_on_where() {
    let form = QueryForm::new();
    assert_eq!(form.mode(), QueryMode::Simple);
    assert_eq!(form.focused_pane(), SimplePane::Where);
    assert_eq!(form.text(SimplePane::Select), "*");
    assert_eq!(form.text(SimplePane::Where), "");
    assert_eq!(form.text(SimplePane::GroupBy), "");
    assert_eq!(form.text(SimplePane::OrderBy), "");
    assert_eq!(form.text(SimplePane::Limit), "1000");
}

#[test]
fn default_compose_yields_select_star_from_t_limit_1000() {
    let form = QueryForm::new();
    let sql = form.to_full_sql(1000).unwrap();
    assert_eq!(sql, "SELECT * FROM t LIMIT 1000");
}

#[test]
fn focus_next_cycles_through_all_five_panes() {
    let mut form = QueryForm::new();
    let order: Vec<SimplePane> = (0..6)
        .map(|_| {
            let p = form.focused_pane();
            form.focus_next();
            p
        })
        .collect();
    // From Where, the cycle is Where -> GroupBy -> OrderBy -> Limit -> Select -> Where
    assert_eq!(
        order,
        vec![
            SimplePane::Where,
            SimplePane::GroupBy,
            SimplePane::OrderBy,
            SimplePane::Limit,
            SimplePane::Select,
            SimplePane::Where,
        ]
    );
}

#[test]
fn focus_prev_cycles_in_reverse() {
    let mut form = QueryForm::new();
    form.focus_prev();
    assert_eq!(form.focused_pane(), SimplePane::Select);
    form.focus_prev();
    assert_eq!(form.focused_pane(), SimplePane::Limit);
}

#[test]
fn focus_pane_jumps_directly_with_click_to_focus() {
    let mut form = QueryForm::new();
    form.focus(SimplePane::OrderBy);
    assert_eq!(form.focused_pane(), SimplePane::OrderBy);
    form.focus(SimplePane::Limit);
    assert_eq!(form.focused_pane(), SimplePane::Limit);
}

#[test]
fn pane_index_round_trips_through_from_index() {
    for pane in SimplePane::ALL {
        assert_eq!(SimplePane::from_index(pane.index()), Some(pane));
    }
    assert_eq!(SimplePane::from_index(5), None);
}

#[test]
fn pane_labels_are_stable_user_facing_strings() {
    assert_eq!(SimplePane::Select.label(), "SELECT");
    assert_eq!(SimplePane::Where.label(), "WHERE");
    assert_eq!(SimplePane::GroupBy.label(), "GROUP BY");
    assert_eq!(SimplePane::OrderBy.label(), "ORDER BY");
    assert_eq!(SimplePane::Limit.label(), "LIMIT");
}

#[test]
fn typing_into_focused_pane_updates_composed_sql() {
    let mut form = QueryForm::new();
    // Default focus is on WHERE — type a predicate.
    form.focused_editor_mut().insert_str("region = 'EU'");
    let sql = form.to_full_sql(1000).unwrap();
    assert_eq!(sql, "SELECT * FROM t WHERE region = 'EU' LIMIT 1000");
}

#[test]
fn toggle_simple_to_power_loads_composed_sql_into_power_editor() {
    let mut form = QueryForm::new();
    form.set_text(SimplePane::Where, "amount > 0");
    form.toggle_mode(1000).unwrap();
    assert_eq!(form.mode(), QueryMode::Power);
    assert_eq!(
        form.power().text(),
        "SELECT * FROM t WHERE amount > 0 LIMIT 1000"
    );
}

#[test]
fn toggle_power_to_simple_distributes_a_clean_select() {
    let mut form = QueryForm::new();
    form.toggle_mode(1000).unwrap(); // -> Power
    form.power_mut()
        .set_text("SELECT id, name FROM t WHERE region = 'EU' LIMIT 50");
    form.toggle_mode(1000).unwrap(); // -> Simple
    assert_eq!(form.mode(), QueryMode::Simple);
    assert_eq!(form.focused_pane(), SimplePane::Where);
    assert_eq!(form.text(SimplePane::Select), "id, name");
    assert_eq!(form.text(SimplePane::Where), "region = 'EU'");
    assert_eq!(form.text(SimplePane::Limit), "50");
}

#[test]
fn toggle_power_to_simple_refuses_a_join_with_a_simplify_error() {
    let mut form = QueryForm::new();
    form.toggle_mode(1000).unwrap();
    form.power_mut()
        .set_text("SELECT * FROM t JOIN u ON t.id = u.id");
    let err = form.toggle_mode(1000).unwrap_err();
    assert_eq!(err.message(), "contains a JOIN");
    // Refusal leaves the form in Power mode (so the user sees the SQL they're working on).
    assert_eq!(form.mode(), QueryMode::Power);
}

#[test]
fn toggle_power_to_simple_with_no_limit_uses_the_default_limit() {
    let mut form = QueryForm::new();
    form.toggle_mode(1000).unwrap();
    form.power_mut().set_text("SELECT * FROM t");
    form.toggle_mode(500).unwrap();
    assert_eq!(form.text(SimplePane::Limit), "500");
}

#[test]
fn limit_error_message_round_trips_through_set_and_clear() {
    let mut form = QueryForm::new();
    assert_eq!(form.limit_error(), None);
    form.set_limit_error(Some("LIMIT must be a number, 'all', or 0".to_string()));
    assert_eq!(
        form.limit_error(),
        Some("LIMIT must be a number, 'all', or 0")
    );
    form.set_limit_error(None);
    assert_eq!(form.limit_error(), None);
}

#[test]
fn set_default_limit_seed_overrides_construction_default() {
    let mut form = QueryForm::new();
    form.set_default_limit_seed(500);
    assert_eq!(form.text(SimplePane::Limit), "500");
}

#[test]
fn set_default_limit_seed_does_not_clobber_user_typed_limit() {
    let mut form = QueryForm::new();
    form.set_text(SimplePane::Limit, "42");
    form.set_default_limit_seed(500);
    assert_eq!(form.text(SimplePane::Limit), "42");
}

#[test]
fn set_default_limit_seed_is_only_applied_once() {
    // Regression: pre-fix the seeder used `if limit.text() == "1000"` which silently re-seeded a
    // user-typed `1000`. Now the first call wins and subsequent calls are no-ops.
    let mut form = QueryForm::new();
    form.set_default_limit_seed(500);
    form.set_default_limit_seed(250);
    assert_eq!(form.text(SimplePane::Limit), "500");
}

#[test]
fn toggle_simple_to_power_with_invalid_limit_refuses_and_preserves_panes() {
    // Regression: pre-fix Simple->Power with an invalid LIMIT silently rewrote the Power buffer
    // to a clean default, discarding the user's typed SELECT/WHERE/GROUP BY/ORDER BY work. Now
    // the toggle is refused, the form stays in Simple mode, and the panes are intact.
    let mut form = QueryForm::new();
    form.set_text(SimplePane::Select, "id, name");
    form.set_text(SimplePane::Where, "region = 'EU'");
    form.set_text(SimplePane::Limit, "1k");
    let err = form.toggle_mode(1000).unwrap_err();
    assert_eq!(err.message(), "LIMIT must be a number, 'all', or 0");
    assert_eq!(form.mode(), QueryMode::Simple);
    assert_eq!(form.text(SimplePane::Select), "id, name");
    assert_eq!(form.text(SimplePane::Where), "region = 'EU'");
    assert_eq!(form.text(SimplePane::Limit), "1k");
    assert_eq!(
        form.limit_error(),
        Some("LIMIT must be a number, 'all', or 0")
    );
}

#[test]
fn enter_power_with_sql_jumps_into_power_mode() {
    let mut form = QueryForm::new();
    form.enter_power_with_sql("SELECT count(*) FROM t");
    assert_eq!(form.mode(), QueryMode::Power);
    assert_eq!(form.power().text(), "SELECT count(*) FROM t");
}

#[test]
fn focus_next_is_a_noop_in_power_mode() {
    let mut form = QueryForm::new();
    form.toggle_mode(1000).unwrap(); // -> Power
    form.focus_next();
    assert_eq!(form.mode(), QueryMode::Power);
}
