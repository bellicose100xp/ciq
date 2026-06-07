//! Tests for `schema::types::ColumnType`.

use crate::schema::types::ColumnType;

#[test]
fn numeric_and_temporal_are_right_aligned() {
    assert!(ColumnType::Int.is_right_aligned());
    assert!(ColumnType::Float.is_right_aligned());
    assert!(ColumnType::Date.is_right_aligned());
    assert!(ColumnType::Timestamp.is_right_aligned());
}

#[test]
fn text_bool_other_are_left_aligned() {
    assert!(!ColumnType::Text.is_right_aligned());
    assert!(!ColumnType::Bool.is_right_aligned());
    assert!(!ColumnType::Other("STRUCT(a INT)".into()).is_right_aligned());
}

#[test]
fn is_numeric_only_int_float() {
    assert!(ColumnType::Int.is_numeric());
    assert!(ColumnType::Float.is_numeric());
    assert!(!ColumnType::Date.is_numeric());
    assert!(!ColumnType::Bool.is_numeric());
    assert!(!ColumnType::Text.is_numeric());
}

#[test]
fn is_temporal_only_date_timestamp() {
    assert!(ColumnType::Date.is_temporal());
    assert!(ColumnType::Timestamp.is_temporal());
    assert!(!ColumnType::Int.is_temporal());
    assert!(!ColumnType::Text.is_temporal());
}

#[test]
fn badges_are_stable_ascii() {
    // ASCII-only, no emoji (theme convention). Stable strings other layers depend on.
    assert_eq!(ColumnType::Int.badge(), "int");
    assert_eq!(ColumnType::Float.badge(), "num");
    assert_eq!(ColumnType::Bool.badge(), "bool");
    assert_eq!(ColumnType::Date.badge(), "date");
    assert_eq!(ColumnType::Timestamp.badge(), "ts");
    assert_eq!(ColumnType::Text.badge(), "txt");
    assert_eq!(ColumnType::Other("BLOB".into()).badge(), "oth");
    for ty in [
        ColumnType::Int,
        ColumnType::Float,
        ColumnType::Bool,
        ColumnType::Date,
        ColumnType::Timestamp,
        ColumnType::Text,
        ColumnType::Other("X".into()),
    ] {
        assert!(ty.badge().is_ascii());
    }
}

#[test]
fn other_preserves_raw_type_string() {
    let ty = ColumnType::Other("DECIMAL(12,2)".into());
    match ty {
        ColumnType::Other(s) => assert_eq!(s, "DECIMAL(12,2)"),
        _ => panic!("expected Other"),
    }
}
