//! Tests for the static DuckDB candidate tables (`dev/PLAN.md` §5.5, `dev/DECISIONS.md` S4).
//!
//! These are const data, so the tests are sanity + the load-bearing **negative** assertion: this
//! is a SQL tool, so jq-only names must be absent. Plus stable-order and well-formedness checks
//! the candidate generator (P3.5) relies on.

use super::*;

#[test]
fn keyword_table_is_nonempty_and_has_core_clauses() {
    for kw in ["SELECT", "FROM", "WHERE", "GROUP BY", "ORDER BY", "LIMIT"] {
        assert!(KEYWORDS.contains(&kw), "missing core clause keyword {kw:?}");
    }
}

#[test]
fn function_table_has_the_five_canonical_aggregates_first() {
    // The §5.4 SelectList row names COUNT/SUM/AVG/MIN/MAX explicitly, and they lead the table in
    // stable order.
    let leading: Vec<&str> = FUNCTIONS.iter().take(5).map(|f| f.name).collect();
    assert_eq!(leading, ["COUNT", "SUM", "AVG", "MIN", "MAX"]);
    assert!(
        FUNCTIONS.iter().take(5).all(|f| f.is_aggregate),
        "the five leading entries are aggregates"
    );
}

#[test]
fn function_table_has_scalar_functions_too() {
    // At least one non-aggregate scalar function exists (so SelectList offers more than aggregates).
    assert!(FUNCTIONS.iter().any(|f| !f.is_aggregate));
    for name in ["lower", "strftime", "date_trunc"] {
        assert!(
            FUNCTIONS.iter().any(|f| f.name == name),
            "missing scalar fn {name:?}"
        );
    }
}

#[test]
fn function_entries_are_well_formed() {
    for f in FUNCTIONS {
        assert!(!f.name.is_empty(), "empty function name");
        assert!(
            f.signature.contains(f.name) || f.signature.contains('('),
            "signature {:?} should look like a call",
            f.signature
        );
        assert!(!f.description.is_empty(), "{} has no description", f.name);
    }
}

#[test]
fn operator_table_covers_the_section_5_4_set() {
    let ops: Vec<&str> = OPERATORS.iter().map(|o| o.text).collect();
    for op in [
        "=",
        "!=",
        "<",
        "<=",
        ">",
        ">=",
        "LIKE",
        "IN",
        "BETWEEN",
        "IS NULL",
        "IS NOT NULL",
    ] {
        assert!(ops.contains(&op), "operator table missing {op:?}");
    }
}

#[test]
fn operator_entries_are_well_formed() {
    for o in OPERATORS {
        assert!(!o.text.is_empty(), "empty operator text");
        assert!(!o.label.is_empty(), "operator {:?} has no label", o.text);
    }
}

/// The load-bearing negative assertion: ciq is a SQL tool, not jq. jq builtins must NOT leak into
/// the SQL candidate tables (the §5.1 "drop jq builtins" decision).
#[test]
fn jq_only_names_are_absent() {
    let jq_only = [
        "to_entries",
        "from_entries",
        "with_entries",
        "gsub",
        "ascii_downcase",
        "splits",
        "getpath",
        "objects",
        "recurse",
        "tostring",
        "tonumber",
    ];
    for name in jq_only {
        assert!(
            !FUNCTIONS.iter().any(|f| f.name.eq_ignore_ascii_case(name)),
            "jq-only builtin {name:?} must not appear in the SQL function table"
        );
        assert!(
            !KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(name)),
            "jq-only builtin {name:?} must not appear in the keyword table"
        );
    }
}

#[test]
fn no_duplicate_function_names() {
    let mut seen = std::collections::HashSet::new();
    for f in FUNCTIONS {
        assert!(seen.insert(f.name), "duplicate function entry {:?}", f.name);
    }
}

#[test]
fn no_duplicate_operators_or_keywords() {
    let mut seen_op = std::collections::HashSet::new();
    for o in OPERATORS {
        assert!(seen_op.insert(o.text), "duplicate operator {:?}", o.text);
    }
    let mut seen_kw = std::collections::HashSet::new();
    for k in KEYWORDS {
        assert!(seen_kw.insert(*k), "duplicate keyword {k:?}");
    }
}
