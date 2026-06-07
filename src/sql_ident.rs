//! SQL identifier quoting — the one place ciq double-quotes a column name for safe
//! interpolation into emitted SQL.
//!
//! Neutral top-level home (sibling of [`sql_lexer`](crate::sql_lexer)) so every emitter
//! (`ingest::csv_opts::to_read_csv_sql`, `autocomplete::value_source::build_distinct_sql`,
//! `autocomplete::insertion`, and later `palette::query_emit`/`facets::facet_query`) shares
//! one escaper rather than each rolling its own. Putting it under `autocomplete/` (its first
//! caller) would force `ingest`/`palette`/`facets` to import the autocomplete module just to
//! quote an identifier — exactly the cross-module coupling the §0/D2 swappable-box discipline
//! avoids.
//!
//! This is the Q3 (`dev/DECISIONS.md`) column-name policy in code: ciq keeps **raw** header
//! names and auto-double-quotes a name on emit when it isn't a bare `[A-Za-z_][A-Za-z0-9_]*`
//! identifier (spaces, special chars, leading digit, empty) or collides with a reserved word.
//! Embedded `"` is escaped by doubling (`"` -> `""`), the SQL standard.

/// Double-quote a SQL identifier and escape any embedded `"` by doubling it. Always quotes —
/// the caller that wants "quote only when needed" uses [`quote_ident_if_needed`].
///
/// `order` -> `"order"`, `we"ird` -> `"we""ird"`, `Total ($)` -> `"Total ($)"`. This keeps a
/// column named after a reserved word, or one containing a quote, from breaking or smuggling
/// into the generated query.
pub fn quote_ident(col: &str) -> String {
    let mut out = String::with_capacity(col.len() + 2);
    out.push('"');
    for ch in col.chars() {
        if ch == '"' {
            out.push('"');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

/// Quote `name` only if it would not re-lex as a bare identifier — i.e. it is empty, starts
/// with a digit, contains a char outside `[A-Za-z0-9_]`, or collides with a reserved keyword.
/// Otherwise returns it verbatim.
///
/// This is the Q3 emit policy: raw names pass through untouched; only names that *need*
/// quoting get it, so common identifiers stay readable in the generated SQL. The
/// reserved-word check defers to [`crate::sql_lexer::is_reserved_keyword`] so "what the lexer
/// would re-read as a keyword" and "what gets quoted on emit" stay in lockstep (one table).
///
/// Note this is *identifier* quoting: a column literally named `*` is quoted as `"*"` (the literal
/// column), NOT left bare. The emitter's own all-columns wildcard is the literal string `*` built
/// directly by its caller ([`crate::palette::query_emit`]'s empty-selection path), which never
/// routes a real column name through here — so there is no live wildcard caller to exempt.
pub fn quote_ident_if_needed(name: &str) -> String {
    if !needs_quoting(name) && !is_reserved_keyword(name) {
        name.to_string()
    } else {
        quote_ident(name)
    }
}

/// Whether `name` is *not* a bare SQL identifier (so it must be double-quoted to be safe):
/// empty, leading digit, or any char outside `[A-Za-z0-9_]`.
pub fn needs_quoting(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        None => true,
        Some(c) if !(c.is_ascii_alphabetic() || c == '_') => true,
        _ => name
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || c == '_')),
    }
}

/// Whether `name` collides with a reserved SQL keyword and so must be double-quoted to be used
/// as a bare identifier. Defers to [`crate::sql_lexer::is_reserved_keyword`] — the single
/// reserved-word table in the crate — so the lexer's `Keyword`-vs-`Ident` classification and
/// the emit-time quoting decision can never drift.
pub fn is_reserved_keyword(name: &str) -> bool {
    crate::sql_lexer::is_reserved_keyword(name)
}

/// Wrap `s` in single quotes, doubling any embedded `'` (the SQL-standard string-literal escape).
/// `O'Brien` -> `'O''Brien'`. The one escaper for every emitted single-quoted string: value
/// literals on insert, `read_csv` string args (path / `delim` / `nullstr` / `dateformat`), and
/// facet/palette predicate values — so quoting can never drift between them.
pub fn single_quote_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push('\'');
        }
        out.push(ch);
    }
    out.push('\'');
    out
}

/// Whether `value` is safe to emit as a **bare** numeric/boolean literal: a `true`/`false` boolean,
/// or a finite number. Any non-numeric text, the empty string, or a non-finite float rendering
/// (`inf`/`-inf`/`NaN`) is **not** bare-safe (DuckDB would read it as an identifier), so the caller
/// single-quotes it instead.
///
/// The single bare-vs-quote predicate shared by every value emitter ([`crate::palette::query_emit`]
/// and [`crate::autocomplete::insertion`]) — so the "is this value safe bare?" decision can no more
/// drift between them than the escapers above can. (It deliberately uses the *stricter* of the two
/// historical copies: only a boolean or finite number is bare; everything else is quoted.)
pub fn is_bare_literal(value: &str) -> bool {
    if value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false") {
        return true;
    }
    value.parse::<f64>().is_ok_and(|f| f.is_finite())
}

/// Render a predicate/insert value with column-type-correct quoting — the single value renderer
/// both emitters share. A numeric (`Int`/`Float`) or `Bool` column whose value is a safe
/// [`is_bare_literal`] emits **bare** (`amount > 5` stays `5`); everything else — a text/temporal/
/// `Other` column, an absent type (`None`), or a numeric column with a non-bare-safe value (a typo,
/// a non-finite float) — single-quotes the value via [`single_quote_literal`]. Quoting a value that
/// "should" be bare is always safe (DuckDB casts the string literal to the column type); emitting a
/// bare token that isn't a real literal is the only unsafe direction, which this never does.
pub fn render_typed_value(value: &str, ty: Option<&crate::schema::ColumnType>) -> String {
    use crate::schema::ColumnType;
    let bare = matches!(
        ty,
        Some(ColumnType::Int | ColumnType::Float | ColumnType::Bool)
    ) && is_bare_literal(value);
    if bare {
        value.to_string()
    } else {
        single_quote_literal(value)
    }
}

#[cfg(test)]
#[path = "sql_ident_tests.rs"]
mod sql_ident_tests;
