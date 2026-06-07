//! A pure CSV dialect sniffer over the leading bytes of a file (`dev/PLAN.md` §6.6, the
//! "Delimiter detect" row of §8/R5).
//!
//! DuckDB has its own (excellent) sniffer that runs at load — this module does **not** replace
//! it. Its job is the small, headless-testable piece ciq can decide without an engine: read the
//! first few lines, pick the most likely **delimiter** (one of `, ; \t |`), the **quote** char,
//! and whether row 0 is a **header**. The result becomes the *sniffed* layer of the
//! [`merge`](super::csv_opts::merge) precedence (CLI > config > sniffed) — i.e. a weak default a
//! user or config can override, and which DuckDB's own detection refines further at load.
//!
//! Pure: a function of bytes, returning a [`SniffResult`]. No I/O beyond the caller handing in the
//! bytes; fully unit-tested over fixed fixtures.

use super::csv_opts::CsvOpts;

/// The delimiter candidates ciq sniffs between, in the documented tie-break order (§6.6): comma
/// first (the overwhelmingly common case and the documented ambiguous-case default), then
/// semicolon, tab, pipe. A tie is broken by this order.
const DELIMITER_CANDIDATES: [char; 4] = [',', ';', '\t', '|'];

/// The number of leading lines the sniffer inspects. Enough to see a consistent delimiter count
/// without scanning a large file; DuckDB's load-time sniffer does the thorough pass.
const SNIFF_LINES: usize = 64;

/// What the sniffer concluded about a CSV's dialect. The `Option` fields mean "could not
/// determine" — the caller turns this into a [`CsvOpts`] where an undetermined field stays `None`
/// (defer to DuckDB).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SniffResult {
    /// The detected field delimiter, or `None` if the input had no usable lines.
    pub delimiter: Option<char>,
    /// The detected quote character (`"` if any quoted field was seen, else `None`).
    pub quote: Option<char>,
    /// Whether the first row looks like a header (non-numeric field names over a numeric-ish body).
    pub header: Option<bool>,
}

impl SniffResult {
    /// Project this sniff into the *sniffed* [`CsvOpts`] layer for [`merge`](super::csv_opts::merge).
    /// Only determined fields become `Some`; the rest stay `None` so a higher layer (or DuckDB)
    /// decides.
    pub fn to_opts(&self) -> CsvOpts {
        CsvOpts {
            delimiter: self.delimiter,
            quote: self.quote,
            header: self.header,
            ..CsvOpts::default()
        }
    }
}

/// Sniff a CSV's dialect from a string slice (convenience over [`sniff_bytes`] for text fixtures).
pub fn sniff(text: &str) -> SniffResult {
    sniff_bytes(text.as_bytes())
}

/// Sniff a CSV's dialect from the leading bytes. Lossily decodes as UTF-8 (a sniff is heuristic;
/// invalid bytes don't matter for delimiter counting), inspects up to [`SNIFF_LINES`] non-empty
/// lines, and:
///  1. picks the delimiter whose per-line field count is the most **consistent** across lines
///     (highest count of lines sharing the modal field count, with `>1` field), tie-broken by
///     [`DELIMITER_CANDIDATES`] order — so a clean comma file beats a stray semicolon;
///  2. sets `quote = Some('"')` iff any `"` appears (DuckDB's default quote char);
///  3. infers a header iff the first row's fields are all non-numeric while at least one later
///     row has a numeric-looking field (a name row over a data body).
pub fn sniff_bytes(bytes: &[u8]) -> SniffResult {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(SNIFF_LINES)
        .collect();

    if lines.is_empty() {
        return SniffResult {
            delimiter: None,
            quote: None,
            header: None,
        };
    }

    let delimiter = detect_delimiter(&lines);
    let quote = if text.contains('"') { Some('"') } else { None };
    let header = delimiter.map(|d| detect_header(&lines, d));

    SniffResult {
        delimiter,
        quote,
        header,
    }
}

/// Choose the delimiter that splits the sampled lines into the most consistent field counts.
///
/// For each candidate we compute the per-line field count (respecting `"`-quoting so a delimiter
/// inside a quoted field doesn't count), then score the candidate by how many lines share the
/// modal field count *when that modal count is `> 1`* (a delimiter that yields one field
/// everywhere isn't really present). The highest score wins; ties break by candidate order, so a
/// file that is equally splittable by comma and pipe is read as comma (the documented default).
/// Returns `None` only if **no** candidate ever produces more than one field (a single-column
/// file) — in which case we still default to comma so the emitted SQL is well-formed.
fn detect_delimiter(lines: &[&str]) -> Option<char> {
    let mut best: Option<(char, usize, usize)> = None; // (delim, score, modal_count)

    for &cand in &DELIMITER_CANDIDATES {
        let counts: Vec<usize> = lines.iter().map(|l| field_count(l, cand)).collect();
        let modal = modal_value(&counts);
        if modal <= 1 {
            continue; // this delimiter doesn't actually split the data
        }
        let score = counts.iter().filter(|&&c| c == modal).count();
        let better = match best {
            None => true,
            Some((_, best_score, _)) => score > best_score, // strict: ties keep the earlier candidate
        };
        if better {
            best = Some((cand, score, modal));
        }
    }

    match best {
        Some((d, _, _)) => Some(d),
        // No delimiter split the data (single-column file): default to comma so SQL is well-formed.
        None => Some(','),
    }
}

/// Count the fields in `line` split on `delim`, honoring `"`-quoting (a `delim` inside a quoted
/// field is part of the field, not a separator). `""` inside a quoted field is an escaped quote.
fn field_count(line: &str, delim: char) -> usize {
    let mut fields = 1;
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            if in_quotes && chars.peek() == Some(&'"') {
                chars.next(); // escaped quote
            } else {
                in_quotes = !in_quotes;
            }
        } else if ch == delim && !in_quotes {
            fields += 1;
        }
    }
    fields
}

/// The most frequent value in `counts` (the mode); ties break toward the larger count so a column
/// count of 3 seen as often as 1 reads as 3. Empty input -> 0.
fn modal_value(counts: &[usize]) -> usize {
    use std::collections::BTreeMap;
    let mut freq: BTreeMap<usize, usize> = BTreeMap::new();
    for &c in counts {
        *freq.entry(c).or_insert(0) += 1;
    }
    freq.into_iter()
        .max_by(|(va, fa), (vb, fb)| fa.cmp(fb).then(va.cmp(vb)))
        .map(|(v, _)| v)
        .unwrap_or(0)
}

/// Infer whether row 0 is a header: true iff every field in row 0 is non-numeric **and** at least
/// one field in a later row is numeric-looking. A file with numeric first-row fields, or with no
/// numeric data at all, is reported as headerless-ambiguous (`false`) — the conservative choice,
/// which DuckDB's own header detection refines at load anyway.
fn detect_header(lines: &[&str], delim: char) -> bool {
    if lines.len() < 2 {
        return false;
    }
    let first = split_fields(lines[0], delim);
    if first.is_empty() || first.iter().any(|f| looks_numeric(f)) {
        return false;
    }
    lines[1..]
        .iter()
        .any(|l| split_fields(l, delim).iter().any(|f| looks_numeric(f)))
}

/// Split a line into trimmed, unquoted field strings on `delim` (quote-aware). Used by header
/// detection; field *counting* uses the cheaper [`field_count`].
fn split_fields(line: &str, delim: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            if in_quotes && chars.peek() == Some(&'"') {
                chars.next();
                cur.push('"');
            } else {
                in_quotes = !in_quotes;
            }
        } else if ch == delim && !in_quotes {
            out.push(cur.trim().to_string());
            cur = String::new();
        } else {
            cur.push(ch);
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// Whether `field` parses as a number (int or float). Empty fields are *not* numeric (an empty
/// header cell shouldn't flip the header guess).
fn looks_numeric(field: &str) -> bool {
    !field.is_empty() && field.parse::<f64>().is_ok()
}

#[cfg(test)]
#[path = "sniff_tests.rs"]
mod sniff_tests;
