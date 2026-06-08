//! Text objects (`iw`/`aw`, `i"`/`a"`, `i(`/`a(`, …) — the pure boundary math.
//!
//! Ported from jiq's `src/editor/text_objects.rs`. Each finder takes a line's text + the cursor
//! **char column** and returns the `(start, end)` char-column span (end exclusive) of the object,
//! or `None` when the cursor isn't inside one. The [`Editor`](crate::app::Editor) cuts that span.
//! SQL value literals (`'EU'`, `"col"`, `(a, b)`) make these directly useful: `ci'` re-types a
//! quoted value, `di(` clears an argument list. Pure `&str + col -> Option<(usize, usize)>`, so it
//! earns the pure-core hard floor (`dev/core-modules.txt`).

use crate::app::editor::mode::TextObjectScope;

/// A text-object target the cursor can sit inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextObjectTarget {
    Word,
    DoubleQuote,
    SingleQuote,
    Backtick,
    Parentheses,
    Brackets,
    Braces,
}

impl TextObjectTarget {
    /// Parse the object char after `i`/`a` (`w`, `"`, `(`/`)`/`b`, …) into a target.
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'w' => Some(TextObjectTarget::Word),
            '"' => Some(TextObjectTarget::DoubleQuote),
            '\'' => Some(TextObjectTarget::SingleQuote),
            '`' => Some(TextObjectTarget::Backtick),
            '(' | ')' | 'b' => Some(TextObjectTarget::Parentheses),
            '[' | ']' => Some(TextObjectTarget::Brackets),
            '{' | '}' | 'B' => Some(TextObjectTarget::Braces),
            _ => None,
        }
    }

    fn delimiters(self) -> Option<(char, char)> {
        match self {
            TextObjectTarget::DoubleQuote => Some(('"', '"')),
            TextObjectTarget::SingleQuote => Some(('\'', '\'')),
            TextObjectTarget::Backtick => Some(('`', '`')),
            TextObjectTarget::Parentheses => Some(('(', ')')),
            TextObjectTarget::Brackets => Some(('[', ']')),
            TextObjectTarget::Braces => Some(('{', '}')),
            TextObjectTarget::Word => None,
        }
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The word boundaries around the cursor (end exclusive). `Inner` is the bare word; `Around`
/// extends over trailing (or, failing that, leading) whitespace, matching vim's `aw`. `None` when
/// the cursor is not on a word character.
pub fn find_word_bounds(
    text: &str,
    cursor_col: usize,
    scope: TextObjectScope,
) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() || cursor_col >= chars.len() {
        return None;
    }
    if !is_word_char(chars[cursor_col]) {
        return None;
    }

    let mut start = cursor_col;
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = cursor_col;
    while end < chars.len() && is_word_char(chars[end]) {
        end += 1;
    }

    match scope {
        TextObjectScope::Inner => Some((start, end)),
        TextObjectScope::Around => {
            if end < chars.len() && chars[end] == ' ' {
                let mut extended_end = end;
                while extended_end < chars.len() && chars[extended_end] == ' ' {
                    extended_end += 1;
                }
                Some((start, extended_end))
            } else if start > 0 && chars[start - 1] == ' ' {
                let mut extended_start = start;
                while extended_start > 0 && chars[extended_start - 1] == ' ' {
                    extended_start -= 1;
                }
                Some((extended_start, end))
            } else {
                Some((start, end))
            }
        }
    }
}

/// The bounds of the same-character-delimited span around the cursor (quotes/backticks). Finds the
/// opening delimiter by counting same-char delimiters before each candidate (even count = an
/// opener), then the next one as the closer. `Inner` excludes the delimiters; `Around` includes
/// them.
pub fn find_quote_bounds(
    text: &str,
    cursor_col: usize,
    delimiter: char,
    scope: TextObjectScope,
) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let cursor_col = cursor_col.min(chars.len().saturating_sub(1));

    let mut open_pos = None;
    for i in (0..=cursor_col).rev() {
        if chars[i] == delimiter {
            let count_before = chars[..i].iter().filter(|&&c| c == delimiter).count();
            if count_before % 2 == 0 {
                open_pos = Some(i);
                break;
            }
        }
    }
    let open = open_pos?;

    let close = chars
        .iter()
        .enumerate()
        .skip(open + 1)
        .find(|&(_, &ch)| ch == delimiter)
        .map(|(i, _)| i)?;

    if cursor_col > close {
        return None;
    }

    match scope {
        TextObjectScope::Inner => Some((open + 1, close)),
        TextObjectScope::Around => Some((open, close + 1)),
    }
}

/// The bounds of the innermost bracket pair containing the cursor (with nesting). `Inner` excludes
/// the brackets; `Around` includes them.
pub fn find_bracket_bounds(
    text: &str,
    cursor_col: usize,
    open_delim: char,
    close_delim: char,
    scope: TextObjectScope,
) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let cursor_col = cursor_col.min(chars.len().saturating_sub(1));

    // When the cursor is on a closing bracket, don't count it toward the depth scan.
    let search_end = if chars[cursor_col] == close_delim {
        cursor_col.saturating_sub(1)
    } else {
        cursor_col
    };

    let mut open_pos = None;
    let mut depth = 0i32;
    for i in (0..=search_end).rev() {
        if chars[i] == close_delim {
            depth += 1;
        } else if chars[i] == open_delim {
            if depth == 0 {
                open_pos = Some(i);
                break;
            }
            depth -= 1;
        }
    }
    let open = open_pos?;

    let mut close_pos = None;
    depth = 0;
    for (i, &ch) in chars.iter().enumerate().skip(open + 1) {
        if ch == open_delim {
            depth += 1;
        } else if ch == close_delim {
            if depth == 0 {
                close_pos = Some(i);
                break;
            }
            depth -= 1;
        }
    }
    let close = close_pos?;

    if cursor_col > close {
        return None;
    }

    match scope {
        TextObjectScope::Inner => Some((open + 1, close)),
        TextObjectScope::Around => Some((open, close + 1)),
    }
}

/// Dispatch to the right finder for `target`.
pub fn find_text_object_bounds(
    text: &str,
    cursor_col: usize,
    target: TextObjectTarget,
    scope: TextObjectScope,
) -> Option<(usize, usize)> {
    match target {
        TextObjectTarget::Word => find_word_bounds(text, cursor_col, scope),
        TextObjectTarget::DoubleQuote => find_quote_bounds(text, cursor_col, '"', scope),
        TextObjectTarget::SingleQuote => find_quote_bounds(text, cursor_col, '\'', scope),
        TextObjectTarget::Backtick => find_quote_bounds(text, cursor_col, '`', scope),
        TextObjectTarget::Parentheses | TextObjectTarget::Brackets | TextObjectTarget::Braces => {
            let (open, close) = target.delimiters()?;
            find_bracket_bounds(text, cursor_col, open, close, scope)
        }
    }
}

#[cfg(test)]
#[path = "text_objects_tests.rs"]
mod text_objects_tests;
