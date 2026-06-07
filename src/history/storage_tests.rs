//! Tests for on-disk history persistence (`storage.rs`) — all against a `tempfile::TempDir`,
//! never `$HOME`. The default-path resolver is not exercised here (it reads the env); these drive
//! the explicit-path load/save/add only.

use super::{add, load, save};
use tempfile::TempDir;

fn temp_file(dir: &TempDir, name: &str) -> std::path::PathBuf {
    dir.path().join(name)
}

#[test]
fn load_missing_file_is_empty() {
    let dir = TempDir::new().unwrap();
    let path = temp_file(&dir, "history");
    assert!(load(&path).is_empty());
}

#[test]
fn save_then_load_round_trips_newest_first() {
    let dir = TempDir::new().unwrap();
    let path = temp_file(&dir, "history");
    let entries = vec!["SELECT 2".to_string(), "SELECT 1".to_string()];
    save(&path, &entries, 100).unwrap();
    assert_eq!(load(&path), entries);
}

#[test]
fn save_creates_parent_dirs() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nested").join("deep").join("history");
    save(&path, &["a".to_string()], 100).unwrap();
    assert_eq!(load(&path), vec!["a".to_string()]);
}

#[test]
fn save_dedupes_keeping_newest() {
    let dir = TempDir::new().unwrap();
    let path = temp_file(&dir, "history");
    save(&path, &["a".into(), "b".into(), "a".into()], 100).unwrap();
    assert_eq!(load(&path), vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn save_trims_to_max() {
    let dir = TempDir::new().unwrap();
    let path = temp_file(&dir, "history");
    let entries: Vec<String> = (0..10).map(|i| format!("q{i}")).collect();
    save(&path, &entries, 3).unwrap();
    // newest-first: keeps the first 3.
    assert_eq!(
        load(&path),
        vec!["q0".to_string(), "q1".to_string(), "q2".to_string()]
    );
}

#[test]
fn load_skips_blank_lines() {
    let dir = TempDir::new().unwrap();
    let path = temp_file(&dir, "history");
    std::fs::write(&path, "a\n\n  \nb\n").unwrap();
    assert_eq!(load(&path), vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn add_moves_existing_to_front() {
    let dir = TempDir::new().unwrap();
    let path = temp_file(&dir, "history");
    save(&path, &["a".into(), "b".into()], 100).unwrap();
    add(&path, "b", 100).unwrap();
    assert_eq!(load(&path), vec!["b".to_string(), "a".to_string()]);
}

#[test]
fn add_inserts_new_at_front() {
    let dir = TempDir::new().unwrap();
    let path = temp_file(&dir, "history");
    add(&path, "first", 100).unwrap();
    add(&path, "second", 100).unwrap();
    assert_eq!(load(&path), vec!["second".to_string(), "first".to_string()]);
}

#[test]
fn add_blank_is_noop() {
    let dir = TempDir::new().unwrap();
    let path = temp_file(&dir, "history");
    add(&path, "   ", 100).unwrap();
    assert!(load(&path).is_empty());
}

#[test]
fn add_respects_max_entries() {
    let dir = TempDir::new().unwrap();
    let path = temp_file(&dir, "history");
    for i in 0..5 {
        add(&path, &format!("q{i}"), 2).unwrap();
    }
    // Only the 2 newest survive (q4, q3).
    assert_eq!(load(&path), vec!["q4".to_string(), "q3".to_string()]);
}
