//! Error enhancement — turn a raw DuckDB error string into a short, friendly status message.
//!
//! `dev/PLAN.md` §3.2: the analog of jiq's jq-error->friendly mapping. Pure
//! `&str -> String`, table-driven tested. DuckDB's messages are already fairly readable, so
//! this trims noise (the `Error: ` prefix, multi-line "LINE n:" carets) and recognizes a few
//! common categories to give a tighter one-liner. When nothing matches, it returns the first
//! line of the original — never empty, never a panic.
//!
//! Two entry points:
//!  - [`enhance`] — the schema-free mapping (the status line default).
//!  - [`enhance_with_schema`] — the same, plus a cheap "did you mean?" suggestion for an unknown
//!    column, matched against the loaded [`Schema`](crate::schema::Schema) by edit distance. The
//!    suggestion is **local and free** (a small bounded Levenshtein over the known headers), never
//!    a second engine query.

use crate::schema::Schema;
use crate::text_match::is_subsequence;

/// Produce a concise, user-facing message from a raw DuckDB error.
pub fn enhance(raw: &str) -> String {
    enhance_inner(raw, None)
}

/// Like [`enhance`], but appends a "did you mean: \"col\"?" hint when the error is an unknown
/// column and a near-match exists among the schema's headers. Used on the App's main-query error
/// path, where the loaded schema is on hand; the suggestion is computed locally (no engine call).
pub fn enhance_with_schema(raw: &str, schema: &Schema) -> String {
    enhance_inner(raw, Some(schema))
}

fn enhance_inner(raw: &str, schema: Option<&Schema>) -> String {
    let first = first_meaningful_line(raw);
    let lower = first.to_ascii_lowercase();

    // A few high-value categories get a friendlier lead-in; everything else passes through
    // as the cleaned first line.
    //
    // Unknown column — DuckDB phrases this as "Referenced column \"x\" not found ..." (and a few
    // close variants); match "column" + "not found" and pull the quoted name.
    if lower.contains("column") && lower.contains("not found") {
        return match between(first, "column \"", "\"") {
            Some(col) => unknown_column_message(col, schema),
            None => "unknown column".to_string(),
        };
    }
    if lower.contains("table") && lower.contains("does not exist") {
        return "unknown table (the loaded CSV is table `t`)".to_string();
    }
    if lower.contains("function")
        && lower.contains("does not exist")
        && let Some(func) = between(first, "name ", " does not exist")
    {
        // DuckDB: "Scalar Function with name <fn> does not exist!" — pull the function name.
        return format!("unknown function: {}", func.trim().trim_matches('"'));
    }
    if lower.contains("syntax error") || lower.contains("parser error") {
        return syntax_error_message(first);
    }
    if lower.contains("conversion") || lower.contains("could not convert") {
        return type_error_message(first);
    }
    if lower.contains("type mismatch") || lower.contains("no function matches") {
        return format!("type error: {}", trim_prefixes(first));
    }
    if lower.contains("ambiguous") && lower.contains("column") {
        return format!("ambiguous column: {}", trim_prefixes(first));
    }
    if lower.contains("division by zero") {
        return "division by zero".to_string();
    }
    if lower.contains("aggregate") && lower.contains("where") {
        // DuckDB: "aggregate function ... is not allowed in the WHERE clause" — a common slip.
        return "aggregates aren't allowed in WHERE (use HAVING)".to_string();
    }
    if lower.contains("interrupt") {
        // Shouldn't normally surface (interrupts map to Cancelled), but be graceful.
        return "query cancelled".to_string();
    }

    trim_prefixes(first)
}

/// The friendly unknown-column message, with an optional "did you mean?" suggestion against the
/// schema headers. Falls back to the bare message when no schema is on hand or no header is close.
fn unknown_column_message(col: &str, schema: Option<&Schema>) -> String {
    let base = format!("unknown column: \"{col}\"");
    match schema.and_then(|s| nearest_column(col, s)) {
        Some(near) => format!("{base} — did you mean \"{near}\"?"),
        None => base,
    }
}

/// The closest schema column to `typo` by a cheap bounded edit distance, or a subsequence match
/// (the user typed a prefix/abbreviation). Returns `None` when nothing is close enough — better no
/// hint than a misleading one. Deterministic: ties break on table order (first column wins).
fn nearest_column<'a>(typo: &str, schema: &'a Schema) -> Option<&'a str> {
    let typo_lower = typo.to_ascii_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for name in schema.names() {
        let name_lower = name.to_ascii_lowercase();
        // A close edit distance OR the typo being a subsequence of the real name both count as a
        // plausible "did you mean". The distance cap scales with length so longer names tolerate a
        // little more drift, but never so much that an unrelated column matches. (No exact-match
        // special-case: this is only reached for an *unknown* column, so the typo never equals a
        // header; a dist-0 match would just re-suggest the same name harmlessly.)
        let dist = levenshtein(&typo_lower, &name_lower);
        let cap = (name_lower.len() / 3).clamp(1, 3);
        let close = dist <= cap || is_subsequence(&name_lower, &typo_lower);
        if close && best.is_none_or(|(d, _)| dist < d) {
            best = Some((dist, name));
        }
    }
    best.map(|(_, name)| name)
}

/// A small, bounded Levenshtein edit distance (insert/delete/substitute, cost 1 each). Operates on
/// chars (so multibyte input never slices a code point). Used only over short column headers, so
/// the quadratic cost is trivial. Empty inputs need no special-case: an empty `a` skips the outer
/// loop and returns `prev = 0..=b.len()`'s tail (`b.len()`), and an empty `b` makes the inner loop
/// a no-op so each row's `cur[0] = i+1` carries through to `a.len()`.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let sub_cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1) // deletion
                .min(cur[j] + 1) // insertion
                .min(prev[j] + sub_cost); // substitution
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// A friendly syntax-error message. When DuckDB pins the error to a token (`syntax error at or
/// near "FROM"`), surface that token; otherwise pass the cleaned line through under a "syntax
/// error:" lead-in.
fn syntax_error_message(first: &str) -> String {
    if let Some(tok) = between(first, "near \"", "\"") {
        // Matches both "syntax error at or near \"X\"" and a bare "near \"X\"" phrasing.
        return format!("syntax error near \"{tok}\"");
    }
    format!("syntax error: {}", trim_prefixes(first))
}

/// A friendly type/conversion-error message. When DuckDB names the value and target type
/// (`Could not convert string 'abc' to INT64`), surface both succinctly; otherwise pass through.
fn type_error_message(first: &str) -> String {
    if let Some(value) = between(first, "convert string '", "'")
        && let Some((_, after_to)) = first.rsplit_once(" to ")
    {
        let target = after_to.trim_end_matches('!').trim();
        if !target.is_empty() {
            return format!("type error: can't read '{value}' as {target}");
        }
    }
    format!("type error: {}", trim_prefixes(first))
}

/// The first non-empty line that carries the actual message (skip blank lines and the
/// `LINE n:`/caret context DuckDB sometimes appends first).
fn first_meaningful_line(raw: &str) -> &str {
    raw.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("LINE ") && !l.starts_with('^'))
        .unwrap_or("")
}

/// Strip leading `Error:` / `Parser Error:` / `Binder Error:` style prefixes.
fn trim_prefixes(s: &str) -> String {
    let mut out = s.trim();
    for p in [
        "Parser Error:",
        "Binder Error:",
        "Catalog Error:",
        "Conversion Error:",
        "Error:",
    ] {
        if let Some(rest) = out.strip_prefix(p) {
            out = rest.trim();
        }
    }
    out.to_string()
}

/// Extract the text between the first `start` and the next `end` after it.
fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    let j = s[i..].find(end)? + i;
    Some(&s[i..j])
}

#[cfg(test)]
#[path = "error_enhance_tests.rs"]
mod error_enhance_tests;
