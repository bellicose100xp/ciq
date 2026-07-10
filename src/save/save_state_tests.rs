//! Tests for the save popup's pure state machine — open/close, filename editing, preview/error.

use super::{PathPreview, SaveState};
use std::path::PathBuf;

#[test]
fn new_is_closed() {
    let s = SaveState::new();
    assert!(!s.is_open());
    assert_eq!(s.filename(), "");
}

#[test]
fn open_prefills_filename_and_clears_error() {
    let mut s = SaveState::new();
    s.set_error("boom");
    s.open("data-out.csv");
    assert!(s.is_open());
    assert_eq!(s.filename(), "data-out.csv");
    assert!(s.error().is_none());
}

#[test]
fn close_clears_everything() {
    let mut s = SaveState::new();
    s.open("x.csv");
    s.set_error("boom");
    s.close();
    assert!(!s.is_open());
    assert_eq!(s.filename(), "");
    assert!(s.error().is_none());
    assert!(s.preview().is_none());
}

#[test]
fn push_pop_edit_filename_and_clear_error() {
    let mut s = SaveState::new();
    s.open("");
    s.set_error("boom");
    s.push('a');
    assert!(s.error().is_none(), "editing clears the stale error");
    s.push('b');
    s.push('c');
    assert_eq!(s.filename(), "abc");
    s.set_error("again");
    s.pop();
    assert_eq!(s.filename(), "ab");
    assert!(s.error().is_none());
}

#[test]
fn preview_round_trips() {
    let mut s = SaveState::new();
    s.open("out.csv");
    assert!(s.preview().is_none());
    s.set_preview(Some(PathPreview {
        path: PathBuf::from("/tmp/out.csv"),
        exists: true,
    }));
    let p = s.preview().expect("preview");
    assert_eq!(p.path, PathBuf::from("/tmp/out.csv"));
    assert!(p.exists);
    s.set_preview(None);
    assert!(s.preview().is_none());
}
