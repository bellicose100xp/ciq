//! Tests for [`EditorMode`] — the default, the `display` labels, and `is_insert`.

use super::*;
use crate::app::editor::char_search::{SearchDirection, SearchType};

#[test]
fn default_is_insert() {
    assert_eq!(EditorMode::default(), EditorMode::Insert);
    assert!(EditorMode::default().is_insert());
}

#[test]
fn display_labels() {
    assert_eq!(EditorMode::Insert.display(), "INSERT");
    assert_eq!(EditorMode::Normal.display(), "NORMAL");
    assert_eq!(EditorMode::Operator('d').display(), "OPERATOR(d)");
    assert_eq!(EditorMode::Operator('c').display(), "OPERATOR(c)");
}

#[test]
fn char_search_display_covers_all_four() {
    assert_eq!(
        EditorMode::CharSearch(SearchDirection::Forward, SearchType::Find).display(),
        "CHAR(f)"
    );
    assert_eq!(
        EditorMode::CharSearch(SearchDirection::Forward, SearchType::Till).display(),
        "CHAR(t)"
    );
    assert_eq!(
        EditorMode::CharSearch(SearchDirection::Backward, SearchType::Find).display(),
        "CHAR(F)"
    );
    assert_eq!(
        EditorMode::CharSearch(SearchDirection::Backward, SearchType::Till).display(),
        "CHAR(T)"
    );
}

#[test]
fn operator_char_search_display() {
    let m = EditorMode::OperatorCharSearch('d', 3, SearchDirection::Forward, SearchType::Find);
    assert_eq!(m.display(), "df");
    let m = EditorMode::OperatorCharSearch('c', 0, SearchDirection::Backward, SearchType::Till);
    assert_eq!(m.display(), "cT");
}

#[test]
fn text_object_display() {
    assert_eq!(
        EditorMode::TextObject('d', TextObjectScope::Inner).display(),
        "di"
    );
    assert_eq!(
        EditorMode::TextObject('c', TextObjectScope::Around).display(),
        "ca"
    );
}

#[test]
fn only_insert_is_insert() {
    assert!(EditorMode::Insert.is_insert());
    assert!(!EditorMode::Normal.is_insert());
    assert!(!EditorMode::Operator('d').is_insert());
    assert!(!EditorMode::CharSearch(SearchDirection::Forward, SearchType::Find).is_insert());
    assert!(!EditorMode::TextObject('d', TextObjectScope::Inner).is_insert());
}
