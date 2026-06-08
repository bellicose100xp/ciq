//! Tests for the pure char-search column math ([`find_char_position`]) and [`SearchDirection`].

use super::*;

#[test]
fn opposite_flips_direction() {
    assert_eq!(
        SearchDirection::Forward.opposite(),
        SearchDirection::Backward
    );
    assert_eq!(
        SearchDirection::Backward.opposite(),
        SearchDirection::Forward
    );
}

// --- forward Find (f) ---

#[test]
fn forward_find_lands_on_target() {
    // "hello world", cursor at 0 ('h'); f'o' -> first 'o' at column 4.
    assert_eq!(
        find_char_position(
            "hello world",
            0,
            'o',
            SearchDirection::Forward,
            SearchType::Find
        ),
        Some(4)
    );
}

#[test]
fn forward_find_skips_the_cursor_char() {
    // Cursor on the first 'o' (col 4); f'o' finds the NEXT 'o' (col 7), not the one under cursor.
    assert_eq!(
        find_char_position(
            "hello world",
            4,
            'o',
            SearchDirection::Forward,
            SearchType::Find
        ),
        Some(7)
    );
}

#[test]
fn forward_find_missing_returns_none() {
    assert_eq!(
        find_char_position("hello", 0, 'z', SearchDirection::Forward, SearchType::Find),
        None
    );
}

#[test]
fn forward_find_at_end_returns_none() {
    // Cursor at last column has nothing to its right.
    assert_eq!(
        find_char_position("ab", 1, 'b', SearchDirection::Forward, SearchType::Find),
        None
    );
}

// --- forward Till (t) ---

#[test]
fn forward_till_stops_one_before_target() {
    // f-till to 'w' in "hello world": 'w' is col 6, till lands at 5 (the space).
    assert_eq!(
        find_char_position(
            "hello world",
            0,
            'w',
            SearchDirection::Forward,
            SearchType::Till
        ),
        Some(5)
    );
}

#[test]
fn forward_till_never_moves_backward() {
    // Cursor at col 3; target 'e' at col 4; till would be 3 (== cursor), so it is clamped up to
    // cursor+1 = 4 (vim's `t` never lands on or before the cursor on a forward search).
    assert_eq!(
        find_char_position("abcde", 3, 'e', SearchDirection::Forward, SearchType::Till),
        Some(4)
    );
}

// --- backward Find (F) ---

#[test]
fn backward_find_lands_on_target() {
    // "hello world", cursor at col 10 ('d'); F'o' -> the 'o' at col 7.
    assert_eq!(
        find_char_position(
            "hello world",
            10,
            'o',
            SearchDirection::Backward,
            SearchType::Find
        ),
        Some(7)
    );
}

#[test]
fn backward_find_at_start_returns_none() {
    assert_eq!(
        find_char_position("abc", 0, 'a', SearchDirection::Backward, SearchType::Find),
        None
    );
}

// --- backward Till (T) ---

#[test]
fn backward_till_stops_one_after_target() {
    // "hello world", cursor at col 10; T'o' -> the 'o' is col 7, till lands at 8.
    assert_eq!(
        find_char_position(
            "hello world",
            10,
            'o',
            SearchDirection::Backward,
            SearchType::Till
        ),
        Some(8)
    );
}

#[test]
fn backward_find_missing_returns_none() {
    assert_eq!(
        find_char_position("abc", 2, 'z', SearchDirection::Backward, SearchType::Find),
        None
    );
}

#[test]
fn multibyte_columns_are_char_indexed() {
    // "café'X" — the quote ' is at char column 4 (é is one char). f' from col 0 -> col 4.
    assert_eq!(
        find_char_position(
            "café'X",
            0,
            '\'',
            SearchDirection::Forward,
            SearchType::Find
        ),
        Some(4)
    );
}
