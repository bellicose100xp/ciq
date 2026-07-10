//! Tests for `save_render` — the pure title/filename/status helpers and the popup blit
//! (`insta` + `ratatui::TestBackend`, logical cells only). True-terminal glyphs, placement, and
//! the green border color are the §4.7 human surface, NOT asserted here.

use super::*;
use crate::save::save_state::{PathPreview, SaveState};
use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

fn render(state: &SaveState, w: u16, h: u16, area: Rect) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).expect("TestBackend");
    t.draw(|f| render_save(state, f, area)).expect("draw");
    t.backend().to_string()
}

fn line_text(line: &ratatui::text::Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[test]
fn title_names_the_action() {
    assert!(title().contains("save"));
    assert!(title().to_lowercase().contains("csv"));
}

#[test]
fn filename_line_shows_input() {
    let mut s = SaveState::new();
    s.open("data-out.csv");
    let text = line_text(&filename_line(&s));
    assert!(text.contains("data-out.csv"), "got: {text}");
}

#[test]
fn status_line_none_without_preview_or_error() {
    let mut s = SaveState::new();
    s.open("");
    assert!(status_line(&s).is_none());
}

#[test]
fn status_line_shows_preview_path() {
    let mut s = SaveState::new();
    s.open("out.csv");
    s.set_preview(Some(PathPreview {
        path: PathBuf::from("/tmp/out.csv"),
        exists: false,
    }));
    let text = line_text(&status_line(&s).expect("preview"));
    assert!(text.contains("/tmp/out.csv"), "got: {text}");
    assert!(
        !text.contains("overwrite"),
        "no warning when absent: {text}"
    );
}

#[test]
fn status_line_warns_on_overwrite() {
    let mut s = SaveState::new();
    s.open("out.csv");
    s.set_preview(Some(PathPreview {
        path: PathBuf::from("/tmp/out.csv"),
        exists: true,
    }));
    let text = line_text(&status_line(&s).expect("preview"));
    assert!(text.contains("overwrites"), "got: {text}");
}

#[test]
fn status_line_shows_error_over_preview() {
    let mut s = SaveState::new();
    s.open("out.csv");
    s.set_preview(Some(PathPreview {
        path: PathBuf::from("/tmp/out.csv"),
        exists: false,
    }));
    s.set_error("cannot write");
    let text = line_text(&status_line(&s).expect("error"));
    assert!(text.contains("cannot write"), "got: {text}");
    assert!(
        !text.contains("/tmp/out.csv"),
        "error hides preview: {text}"
    );
}

#[test]
fn popup_shows_filename_and_title() {
    let mut s = SaveState::new();
    s.open("report.csv");
    let screen = render(&s, 60, 8, Rect::new(0, 0, 50, 4));
    assert!(screen.contains("save"), "screen:\n{screen}");
    assert!(screen.contains("report.csv"), "screen:\n{screen}");
}

#[test]
fn snapshot_save_popup_80x24() {
    let mut s = SaveState::new();
    s.open("showcase-m-out.csv");
    s.set_preview(Some(PathPreview {
        path: PathBuf::from("showcase-m-out.csv"),
        exists: false,
    }));
    let screen = render(&s, 80, 24, Rect::new(0, 1, 48, 4));
    insta::assert_snapshot!(screen);
}
