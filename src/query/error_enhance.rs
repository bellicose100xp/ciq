//! Error enhancement — turn a raw DuckDB error string into a short, friendly status message.
//!
//! `dev/PLAN.md` §3.2: the analog of jiq's jq-error->friendly mapping. Pure
//! `&str -> String`, table-driven tested. DuckDB's messages are already fairly readable, so
//! this trims noise (the `Error: ` prefix, multi-line "LINE n:" carets) and recognizes a few
//! common categories to give a tighter one-liner. When nothing matches, it returns the first
//! line of the original — never empty, never a panic.

/// Produce a concise, user-facing message from a raw DuckDB error.
pub fn enhance(raw: &str) -> String {
    let first = first_meaningful_line(raw);
    let lower = first.to_ascii_lowercase();

    // A few high-value categories get a friendlier lead-in; everything else passes through
    // as the cleaned first line.
    if lower.contains("referenced column") && lower.contains("not found") {
        return match between(first, "column \"", "\"") {
            Some(col) => format!("unknown column: \"{col}\""),
            None => "unknown column".to_string(),
        };
    }
    if lower.contains("table") && lower.contains("does not exist") {
        return "unknown table (the loaded CSV is table `t`)".to_string();
    }
    if lower.contains("syntax error") || lower.contains("parser error") {
        return format!("syntax error: {}", trim_prefixes(first));
    }
    if lower.contains("conversion") || lower.contains("could not convert") {
        return format!("type error: {}", trim_prefixes(first));
    }
    if lower.contains("interrupt") {
        // Shouldn't normally surface (interrupts map to Cancelled), but be graceful.
        return "query cancelled".to_string();
    }

    trim_prefixes(first)
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
