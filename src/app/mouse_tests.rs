//! Tests for the headless [`MouseEvent`] model — the `position()` projection across every kind.

use super::MouseEvent;

#[test]
fn position_reads_back_each_kind() {
    assert_eq!(MouseEvent::ScrollUp { col: 1, row: 2 }.position(), (1, 2));
    assert_eq!(MouseEvent::ScrollDown { col: 3, row: 4 }.position(), (3, 4));
    assert_eq!(MouseEvent::ScrollLeft { col: 5, row: 6 }.position(), (5, 6));
    assert_eq!(
        MouseEvent::ScrollRight { col: 7, row: 8 }.position(),
        (7, 8)
    );
    assert_eq!(MouseEvent::Click { col: 9, row: 10 }.position(), (9, 10));
    assert_eq!(MouseEvent::Drag { col: 11, row: 12 }.position(), (11, 12));
}
