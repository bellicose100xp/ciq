//! Suggestion data + the tolerant parser for the model's JSON reply.
//!
//! The system prompt ([`super::chat_prompt`]) asks for exactly one JSON object carrying the prose
//! `reply` the user reads plus any `suggestions`, but the model may still wrap it in a code fence or
//! surround it with prose. [`parse_reply`] is deliberately forgiving about *packaging* (fences,
//! prose, stray braces inside strings) while staying strict about *content*: an unknown `type` or
//! blank `sql` drops the entry, and every surviving `sql` is re-checked by [`keep_valid`] through
//! the same read-only guard a typed query goes through. Pure and deterministic; no I/O.

use crate::ai::prompt::strip_code_fences;
use crate::query::preprocess::prepare_interactive;
use serde::Deserialize;
use serde_json::Value;

/// Maximum suggestions kept (extra entries are dropped, in order).
pub const MAX_SUGGESTIONS: usize = 5;

/// The category the model assigned to a suggestion. There is deliberately no "next query"
/// category: predicting what the user wants to look at next proved to be noise they never picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionKind {
    /// Repairs the current (failing) query.
    Fix,
    /// A faster or cleaner equivalent of the current query, or the query the user just asked for.
    Optimize,
}

impl SuggestionKind {
    /// The wire label the model uses for this kind.
    pub fn label(self) -> &'static str {
        match self {
            SuggestionKind::Fix => "fix",
            SuggestionKind::Optimize => "optimize",
        }
    }

    /// Parse a wire label, case-insensitively and ignoring surrounding whitespace. An unknown label
    /// (including the retired `next`) yields `None`, so the entry is dropped.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        [Self::Fix, Self::Optimize]
            .into_iter()
            .find(|k| k.label().eq_ignore_ascii_case(s))
    }
}

/// One suggestion: a read-only query the user can pick, plus a one-line explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub kind: SuggestionKind,
    pub sql: String,
    pub details: String,
}

/// One parsed assistant answer: the prose the user reads plus any pickable queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatReply {
    /// The model's plain-language answer. Never empty — a reply with no prose falls back to a
    /// generic line so the transcript never shows a blank turn.
    pub reply: String,
    /// The pickable queries, already normalized and capped.
    pub suggestions: Vec<Suggestion>,
}

/// What [`parse_reply`] made of the raw completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseOutcome {
    /// A well-formed reply object was found.
    Parsed(ChatReply),
    /// No reply object could be located in the completion.
    Unparseable,
}

/// The `{"reply": "...", "suggestions": [...]}` envelope. `reply` is optional so an older-shaped
/// answer that carries only suggestions still parses, and entries stay as raw JSON so one malformed
/// entry (e.g. a numeric `sql`) is skipped instead of failing the whole reply.
#[derive(Deserialize)]
struct RawReply {
    #[serde(default)]
    reply: Option<String>,
    #[serde(default)]
    suggestions: Vec<Value>,
}

/// Tolerant parse of the model reply.
///
/// Unwraps one outer code fence, tries the whole text as JSON, then falls back to the first
/// balanced `{ ... }` object (string-escape aware) that carries a `reply` or `suggestions` key.
/// Entries are normalized (trimmed `sql` with one trailing `;` removed, trimmed `details`, missing
/// `details` -> `""`), entries with an unknown `type` or blank `sql` are dropped, duplicates by
/// exact `sql` keep the first occurrence, and the list is capped at [`MAX_SUGGESTIONS`].
///
/// An empty suggestion list is a normal answer here (the user may have asked a question that needs
/// no query), so there is no separate "empty" outcome — only "could not find an object at all".
pub fn parse_reply(raw: &str) -> ParseOutcome {
    let text = strip_code_fences(raw);
    let Some(found) = locate_reply(&text) else {
        return ParseOutcome::Unparseable;
    };
    let suggestions = normalize(found.suggestions);
    let reply = found
        .reply
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| default_prose(&suggestions));
    ParseOutcome::Parsed(ChatReply { reply, suggestions })
}

/// The prose shown when the model returned suggestions but no `reply` text.
fn default_prose(suggestions: &[Suggestion]) -> String {
    match suggestions.len() {
        0 => "no answer".to_string(),
        1 => "here is one query".to_string(),
        n => format!("here are {n} queries"),
    }
}

/// Keep only suggestions whose `sql` passes the interactive read-only single-`SELECT` guard.
/// Order is preserved.
pub fn keep_valid(suggestions: Vec<Suggestion>) -> Vec<Suggestion> {
    suggestions
        .into_iter()
        .filter(|s| prepare_interactive(&s.sql, None).is_ok())
        .collect()
}

fn locate_reply(text: &str) -> Option<RawReply> {
    // Require a JSON *object* that actually names one of our keys. Both guards earn their place:
    // serde deserializes a struct from a JSON array positionally, so a bare `["suggestions"]` would
    // otherwise land as a reply; and with both fields defaulted, any unrelated object would parse.
    let from_object = |candidate: &str| -> Option<RawReply> {
        let value: Value = serde_json::from_str(candidate).ok()?;
        let map = value.as_object()?;
        if !map.contains_key("reply") && !map.contains_key("suggestions") {
            return None;
        }
        serde_json::from_value::<RawReply>(value).ok()
    };
    if let Some(raw) = from_object(text) {
        return Some(raw);
    }
    text.char_indices()
        .filter(|&(_, c)| c == '{')
        .filter_map(|(start, _)| balanced_object(&text[start..]))
        .find_map(from_object)
}

/// The shortest prefix of `s` (which must start at a `{`) that forms a brace-balanced object,
/// skipping braces that appear inside JSON strings. `None` if the braces never balance.
fn balanced_object(s: &str) -> Option<&str> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if in_string {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[..i + c.len_utf8()]);
                }
            }
            _ => {}
        }
    }
    None
}

fn normalize(entries: Vec<Value>) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    for entry in entries {
        let Some(suggestion) = normalize_entry(&entry) else {
            continue;
        };
        if out.iter().any(|s| s.sql == suggestion.sql) {
            continue;
        }
        out.push(suggestion);
        if out.len() == MAX_SUGGESTIONS {
            break;
        }
    }
    out
}

fn normalize_entry(entry: &Value) -> Option<Suggestion> {
    let kind = SuggestionKind::parse(entry.get("type")?.as_str()?)?;
    let sql = entry.get("sql")?.as_str()?.trim();
    let sql = sql.strip_suffix(';').unwrap_or(sql).trim();
    if sql.is_empty() {
        return None;
    }
    let details = entry
        .get("details")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    Some(Suggestion {
        kind,
        sql: sql.to_string(),
        details,
    })
}

#[cfg(test)]
#[path = "suggestion_tests.rs"]
mod suggestion_tests;
