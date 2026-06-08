//! Tests for the pure text-object boundary math (word / quote / bracket finders) and target parse.

use super::*;
use crate::app::editor::mode::TextObjectScope::{Around, Inner};

// --- target parse ---

#[test]
fn from_char_maps_targets() {
    assert_eq!(
        TextObjectTarget::from_char('w'),
        Some(TextObjectTarget::Word)
    );
    assert_eq!(
        TextObjectTarget::from_char('"'),
        Some(TextObjectTarget::DoubleQuote)
    );
    assert_eq!(
        TextObjectTarget::from_char('\''),
        Some(TextObjectTarget::SingleQuote)
    );
    assert_eq!(
        TextObjectTarget::from_char('`'),
        Some(TextObjectTarget::Backtick)
    );
    // Open, close, and the `b` alias all map to parens.
    for c in ['(', ')', 'b'] {
        assert_eq!(
            TextObjectTarget::from_char(c),
            Some(TextObjectTarget::Parentheses)
        );
    }
    for c in ['[', ']'] {
        assert_eq!(
            TextObjectTarget::from_char(c),
            Some(TextObjectTarget::Brackets)
        );
    }
    for c in ['{', '}', 'B'] {
        assert_eq!(
            TextObjectTarget::from_char(c),
            Some(TextObjectTarget::Braces)
        );
    }
    assert_eq!(TextObjectTarget::from_char('z'), None);
}

// --- word bounds ---

#[test]
fn inner_word_bounds() {
    // "foo bar baz", cursor in "bar" (col 5) -> [4, 7).
    assert_eq!(find_word_bounds("foo bar baz", 5, Inner), Some((4, 7)));
}

#[test]
fn around_word_extends_over_trailing_space() {
    // "foo bar baz", cursor in "bar" -> around extends to include the trailing space: [4, 8).
    assert_eq!(find_word_bounds("foo bar baz", 5, Around), Some((4, 8)));
}

#[test]
fn around_word_extends_over_leading_space_when_no_trailing() {
    // "foo bar", cursor in "bar" (the last word, no trailing space) -> include the leading space.
    assert_eq!(find_word_bounds("foo bar", 5, Around), Some((3, 7)));
}

#[test]
fn word_bounds_none_off_a_word_char() {
    // Cursor on the space at col 3 -> not on a word char.
    assert_eq!(find_word_bounds("foo bar", 3, Inner), None);
}

#[test]
fn word_bounds_underscore_is_a_word_char() {
    // SQL identifiers contain underscores: "order_id" is one word.
    assert_eq!(find_word_bounds("order_id = 1", 3, Inner), Some((0, 8)));
}

// --- quote bounds (SQL string literals) ---

#[test]
fn inner_single_quote_bounds() {
    // "x = 'EU'", cursor inside the literal (col 6, 'U') -> content [5, 7).
    assert_eq!(find_quote_bounds("x = 'EU'", 6, '\'', Inner), Some((5, 7)));
}

#[test]
fn around_single_quote_includes_delimiters() {
    assert_eq!(find_quote_bounds("x = 'EU'", 6, '\'', Around), Some((4, 8)));
}

#[test]
fn quote_bounds_none_outside_any_pair() {
    // Cursor before the opening quote.
    assert_eq!(find_quote_bounds("x = 'EU'", 0, '\'', Inner), None);
}

#[test]
fn double_quote_identifier_bounds() {
    // Quoted SQL identifier: `select "My Col"` — cursor inside, inner is the name.
    assert_eq!(find_quote_bounds("\"My Col\"", 3, '"', Inner), Some((1, 7)));
}

// --- bracket bounds (with nesting) ---

#[test]
fn inner_paren_bounds() {
    // "f(a, b)", cursor inside the args (col 3) -> content [2, 6).
    assert_eq!(
        find_bracket_bounds("f(a, b)", 3, '(', ')', Inner),
        Some((2, 6))
    );
}

#[test]
fn around_paren_includes_delimiters() {
    assert_eq!(
        find_bracket_bounds("f(a, b)", 3, '(', ')', Around),
        Some((1, 7))
    );
}

#[test]
fn nested_paren_finds_innermost() {
    // "a(b(c)d)", cursor on 'c' (col 4) -> innermost pair [4, 5).
    assert_eq!(
        find_bracket_bounds("a(b(c)d)", 4, '(', ')', Inner),
        Some((4, 5))
    );
}

#[test]
fn cursor_on_closing_bracket_resolves_pair() {
    // "(ab)", cursor on the closing ')' (col 3) -> inner content [1, 3).
    assert_eq!(
        find_bracket_bounds("(ab)", 3, '(', ')', Inner),
        Some((1, 3))
    );
}

#[test]
fn bracket_bounds_none_outside_pair() {
    assert_eq!(find_bracket_bounds("abc", 1, '(', ')', Inner), None);
}

// --- dispatch ---

// --- edge cases (empty / out-of-range / fallbacks / nesting) ---

#[test]
fn word_bounds_empty_and_out_of_range() {
    assert_eq!(find_word_bounds("", 0, Inner), None);
    assert_eq!(find_word_bounds("ab", 9, Inner), None);
}

#[test]
fn around_word_single_word_no_surrounding_space() {
    // The whole buffer is one word with no spaces — around falls back to the bare word.
    assert_eq!(find_word_bounds("word", 1, Around), Some((0, 4)));
}

#[test]
fn quote_bounds_empty_and_missing_close() {
    assert_eq!(find_quote_bounds("", 0, '"', Inner), None);
    // An opening quote with no closer -> None.
    assert_eq!(find_quote_bounds("a 'b c", 3, '\'', Inner), None);
}

#[test]
fn quote_bounds_cursor_past_close_is_none() {
    // "'a' x" — cursor at col 4 ('x'), which is after the closed pair -> None (cursor not inside).
    assert_eq!(find_quote_bounds("'a' x", 4, '\'', Inner), None);
}

#[test]
fn bracket_bounds_empty_input() {
    assert_eq!(find_bracket_bounds("", 0, '(', ')', Inner), None);
}

#[test]
fn bracket_bounds_no_open_is_none() {
    // Only a closing bracket to the left of the cursor, no opener -> None.
    assert_eq!(find_bracket_bounds("ab)c", 3, '(', ')', Inner), None);
}

#[test]
fn bracket_bounds_depth_decrement_skips_inner_pair() {
    // "((a)b)" cursor on 'b' (col 4): scanning left, the inner ")" at col 3 raises depth, the inner
    // "(" at col 1 drops it back, so the matched opener is the OUTER "(" at col 0 -> inner [1, 5).
    assert_eq!(
        find_bracket_bounds("((a)b)", 4, '(', ')', Inner),
        Some((1, 5))
    );
}

#[test]
fn bracket_bounds_cursor_past_close_is_none() {
    // "(a) z" — cursor at col 4 ('z'), past the matched pair -> None.
    assert_eq!(find_bracket_bounds("(a) z", 4, '(', ')', Inner), None);
}

#[test]
fn dispatch_double_quote_and_word_arms() {
    // Exercise the DoubleQuote + Word dispatch arms explicitly (Around scope too).
    assert_eq!(
        find_text_object_bounds("\"hi\"", 1, TextObjectTarget::DoubleQuote, Around),
        Some((0, 4))
    );
    assert_eq!(
        find_text_object_bounds("ab cd", 4, TextObjectTarget::Word, Around),
        Some((2, 5))
    );
}

#[test]
fn dispatch_routes_each_target() {
    assert_eq!(
        find_text_object_bounds("foo bar", 0, TextObjectTarget::Word, Inner),
        Some((0, 3))
    );
    assert_eq!(
        find_text_object_bounds("'EU'", 1, TextObjectTarget::SingleQuote, Inner),
        Some((1, 3))
    );
    assert_eq!(
        find_text_object_bounds("(x)", 1, TextObjectTarget::Parentheses, Around),
        Some((0, 3))
    );
    assert_eq!(
        find_text_object_bounds("[x]", 1, TextObjectTarget::Brackets, Inner),
        Some((1, 2))
    );
    assert_eq!(
        find_text_object_bounds("{x}", 1, TextObjectTarget::Braces, Inner),
        Some((1, 2))
    );
    assert_eq!(
        find_text_object_bounds("`x`", 1, TextObjectTarget::Backtick, Inner),
        Some((1, 2))
    );
}
