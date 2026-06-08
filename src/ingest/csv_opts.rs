//! `CsvOpts` — the CSV dialect / type-override option set, the precedence [`merge`], and the
//! [`to_read_csv_sql`] builder. (`dev/PLAN.md` §6.6 + §8/R5; `dev/DECISIONS.md` Q3/Q7/Q12 + the
//! "CsvOpts <-> CLI-flag inventory" item.)
//!
//! Every field is `Option<_>`: `None` means "no override at this layer — defer to the layer
//! below" (config, then sniffed, then DuckDB's own auto-detect). The full-`None` `Default` is the
//! common path: let DuckDB auto-detect everything, scanning the whole file for types.

use crate::sql_ident::single_quote_literal;

/// CSV ingest / dialect + type-override options.
///
/// Three instances are merged at load by [`merge`] (CLI > config > sniffed). Each field documents
/// its `read_csv` mapping; [`to_read_csv_sql`] emits them.
///
/// Field <-> CLI-flag <-> `read_csv` arg inventory (the R5 reconciliation, closed here):
///
/// | field | CLI flag | `read_csv(...)` arg |
/// |---|---|---|
/// | `delimiter` | `--delim` | `delim='<c>'` |
/// | `quote` | `--quote` | `quote='<c>'` |
/// | `escape` | `--escape` | `escape='<c>'` |
/// | `header` | `--header` / `--no-header` | `header=true|false` |
/// | `null_string` | `--null-string` | `nullstr='<s>'` |
/// | `sample_size` | `--sample-size` (was `--sniff-rows`) | `sample_size=<n>` |
/// | `types` | `--types 'a=VARCHAR,b=DECIMAL(12,2)'` | `types={'a': 'VARCHAR', ...}` |
/// | `all_varchar` | `--all-varchar` | `all_varchar=true` |
/// | `date_format` | `--date-format '%d/%m/%Y'` | `dateformat='<f>'` |
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CsvOpts {
    /// Field delimiter. `read_csv(delim='<c>')`. `None` = auto-detect / sniffed.
    pub delimiter: Option<char>,
    /// Quote character. `read_csv(quote='<c>')`. `None` = auto-detect.
    pub quote: Option<char>,
    /// Escape character inside a quoted field. `read_csv(escape='<c>')`. `None` = auto-detect.
    pub escape: Option<char>,
    /// Whether the first row is a header. `read_csv(header=true|false)`. `None` = auto-detect.
    pub header: Option<bool>,
    /// String(s) that ingest as SQL `NULL` (the Q12 user lever). `read_csv(nullstr='<s>')`.
    /// `None` = DuckDB default (unquoted empty -> NULL; quoted `""` -> empty string).
    pub null_string: Option<String>,
    /// Rows the type sniffer samples. `read_csv(sample_size=<n>)`. **Unifies `--sniff-rows`**
    /// (R5) with `sample_size`: one name. `Some(-1)` scans the whole file (the default-load
    /// behavior preserved when `None`); `None` lets [`to_read_csv_sql`] apply the `-1` default.
    pub sample_size: Option<i64>,
    /// Per-column type overrides, bypassing the sniffer (`--types`). `read_csv(types={...})`.
    /// Ordered + de-duplicated by [`merge`] so the emitted SQL is deterministic.
    pub types: Option<Vec<ColumnTypeOverride>>,
    /// Ingest every column as `VARCHAR` (`--all-varchar`) — the no-semantic-surprise escape
    /// hatch. `read_csv(all_varchar=true)`. `None`/`Some(false)` = sniff normally.
    pub all_varchar: Option<bool>,
    /// Explicit date parse format for ambiguous locale dates (`--date-format`).
    /// `read_csv(dateformat='<f>')`. `None` = DuckDB's own date detection.
    pub date_format: Option<String>,
}

/// One `column = TYPE` entry of the `--types` override. `name` is the **raw** header name (Q3:
/// kept verbatim, quoted only on emit); `sql_type` is the DuckDB type text the user supplied
/// (e.g. `VARCHAR`, `DECIMAL(12,2)`), passed through to DuckDB which validates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnTypeOverride {
    pub name: String,
    pub sql_type: String,
}

impl ColumnTypeOverride {
    pub fn new(name: impl Into<String>, sql_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sql_type: sql_type.into(),
        }
    }
}

impl CsvOpts {
    /// Whether any field is set — i.e. whether [`to_read_csv_sql`] must emit explicit `read_csv`
    /// args at all (vs the all-`None` `read_csv_auto` fast path).
    fn has_any_override(&self) -> bool {
        self.delimiter.is_some()
            || self.quote.is_some()
            || self.escape.is_some()
            || self.header.is_some()
            || self.null_string.is_some()
            || self.sample_size.is_some()
            || self.types.as_ref().is_some_and(|t| !t.is_empty())
            || self.all_varchar.is_some()
            || self.date_format.is_some()
    }
}

/// Merge three option layers into one effective [`CsvOpts`], field by field, with precedence
/// **CLI > config > sniffed**. For each field the first `Some` wins in that order; if all three
/// are `None` the result is `None` (defer to DuckDB auto-detect).
///
/// Pure — no I/O — so it is exhaustively unit-tested. The `types` vector is normalized
/// (de-duplicated keeping the last write per column, then sorted by column name) so the emitted
/// SQL is byte-stable regardless of input order (the determinism rule for user-visible output).
pub fn merge(config: &CsvOpts, cli: &CsvOpts, sniffed: &CsvOpts) -> CsvOpts {
    fn pick<T: Clone>(cli: &Option<T>, config: &Option<T>, sniffed: &Option<T>) -> Option<T> {
        cli.clone()
            .or_else(|| config.clone())
            .or_else(|| sniffed.clone())
    }

    let types = pick(&cli.types, &config.types, &sniffed.types).map(|t| normalize_types(&t));

    CsvOpts {
        delimiter: pick(&cli.delimiter, &config.delimiter, &sniffed.delimiter),
        quote: pick(&cli.quote, &config.quote, &sniffed.quote),
        escape: pick(&cli.escape, &config.escape, &sniffed.escape),
        header: pick(&cli.header, &config.header, &sniffed.header),
        null_string: pick(&cli.null_string, &config.null_string, &sniffed.null_string),
        sample_size: pick(&cli.sample_size, &config.sample_size, &sniffed.sample_size),
        types,
        all_varchar: pick(&cli.all_varchar, &config.all_varchar, &sniffed.all_varchar),
        date_format: pick(&cli.date_format, &config.date_format, &sniffed.date_format),
    }
}

/// De-duplicate (last write per column name wins) and sort type overrides by column name, so the
/// emitted `types={...}` map is deterministic regardless of CLI/config ordering. `merge` applies
/// it once; `to_read_csv_sql` applies it again so emit is self-deterministic even for a `CsvOpts`
/// built directly (not via `merge`) — idempotent, so a second pass is a no-op.
fn normalize_types(overrides: &[ColumnTypeOverride]) -> Vec<ColumnTypeOverride> {
    // Last write wins: walk in order, recording the final type per name.
    use std::collections::BTreeMap;
    let mut by_name: BTreeMap<&str, &str> = BTreeMap::new();
    for o in overrides {
        by_name.insert(o.name.as_str(), o.sql_type.as_str());
    }
    by_name
        .into_iter()
        .map(|(name, sql_type)| ColumnTypeOverride::new(name, sql_type))
        .collect()
}

/// Build the `CREATE TABLE` source expression — the `read_csv(...)` (or `read_csv_auto(...)`)
/// call — for `path` under the effective `opts`.
///
/// When `opts` has **no** overrides, emits the auto-detect form with the whole-file type scan:
/// `read_csv_auto('<path>', sample_size = -1)`. This is byte-identical to the pre-ingest default,
/// so the engine's existing goldens (the `created_at -> DATE` test) stay green.
///
/// When any override is set, emits the explicit form: `read_csv('<path>', auto_detect=true,
/// <args>)`. `auto_detect=true` keeps DuckDB's sniffer on for the fields the user did *not*
/// override (a partial override doesn't disable the rest of the detection).
///
/// Quoting / escaping:
///  - the path and all string-valued args (`delim`, `quote`, `nullstr`, `dateformat`) are
///    single-quoted with `'`-doubling via [`single_quote_literal`];
///  - column names in `types={...}` are **raw** header names (Q3) quoted as SQL string keys (the
///    `types` map keys are strings, not identifiers, in DuckDB) via `'`-doubling;
///  - the user-supplied type text is passed through inside a `'`-quoted string (DuckDB validates
///    it; an invalid type surfaces as a clean `EngineError::Load`, never a panic).
///
/// Pure: returns a `String`, runs nothing. Golden-snapshot-tested byte-for-byte.
pub fn to_read_csv_sql(opts: &CsvOpts, path: &str) -> String {
    let path_lit = single_quote_literal(path);

    if !opts.has_any_override() {
        // Auto-detect fast path: whole-file type scan, byte-identical to the original default.
        return format!("read_csv_auto({path_lit}, sample_size = -1)");
    }

    let mut args: Vec<String> = vec!["auto_detect = true".to_string()];

    if let Some(d) = opts.delimiter {
        args.push(format!("delim = {}", single_quote_literal(&d.to_string())));
    }
    if let Some(q) = opts.quote {
        args.push(format!("quote = {}", single_quote_literal(&q.to_string())));
    }
    if let Some(e) = opts.escape {
        args.push(format!("escape = {}", single_quote_literal(&e.to_string())));
    }
    if let Some(h) = opts.header {
        args.push(format!("header = {h}"));
    }
    if let Some(ns) = &opts.null_string {
        args.push(format!("nullstr = {}", single_quote_literal(ns)));
    }
    // sample_size: an explicit override emits as given; absent here means the override path didn't
    // touch it, so fall back to the whole-file scan (-1) to preserve the default-load behavior.
    let sample = opts.sample_size.unwrap_or(-1);
    args.push(format!("sample_size = {sample}"));

    if opts.all_varchar == Some(true) {
        args.push("all_varchar = true".to_string());
    }
    if let Some(df) = &opts.date_format {
        args.push(format!("dateformat = {}", single_quote_literal(df)));
    }
    if let Some(types) = &opts.types
        && !types.is_empty()
    {
        // Normalize at emit so the `types={...}` map is byte-stable even for a `CsvOpts` built
        // directly (not via `merge`); idempotent if `merge` already normalized it.
        let normalized = normalize_types(types);
        let entries: Vec<String> = normalized
            .iter()
            .map(|o| {
                format!(
                    "{}: {}",
                    single_quote_literal(&o.name),
                    single_quote_literal(&o.sql_type)
                )
            })
            .collect();
        args.push(format!("types = {{{}}}", entries.join(", ")));
    }

    format!("read_csv({path_lit}, {})", args.join(", "))
}

/// Parse a `--types` spec — `'name=TYPE[,name=TYPE]...'` — into [`ColumnTypeOverride`]s.
///
/// Splits on **top-level** commas only, so a parameterized type with its own comma
/// (`DECIMAL(12,2)`) stays one entry. A blank entry is skipped; an entry without `=`, or with an
/// empty name/type, is an error (surfaced to the user, not silently dropped). Pure — lives here so
/// it is unit-tested in the library rather than the binary.
pub fn parse_types_spec(spec: &str) -> Result<Vec<ColumnTypeOverride>, String> {
    let mut out = Vec::new();
    for entry in split_top_level_commas(spec) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (name, ty) = entry
            .split_once('=')
            .ok_or_else(|| format!("invalid --types entry '{entry}' (expected name=TYPE)"))?;
        let name = name.trim();
        let ty = ty.trim();
        if name.is_empty() || ty.is_empty() {
            return Err(format!(
                "invalid --types entry '{entry}' (empty name or type)"
            ));
        }
        out.push(ColumnTypeOverride::new(name, ty));
    }
    Ok(out)
}

/// Split `spec` on commas that are not inside parentheses, so `DECIMAL(12,2)` stays intact.
fn split_top_level_commas(spec: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut depth: i32 = 0;
    for ch in spec.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(ch);
            }
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut cur));
            }
            _ => cur.push(ch),
        }
    }
    parts.push(cur);
    parts
}

/// The compact delimiter/header indicator shown in the results-pane border title, e.g.
/// `delim , | header on`. Pure: built from the active dialect the App holds.
///
/// `delimiter` is `None` when DuckDB auto-detected it (shown as `auto`); `header` reflects whether
/// the first row was treated as a header. ASCII only (no emoji); the literal delimiter glyph is
/// shown verbatim (a tab is shown as `\t` so it stays visible).
pub fn dialect_summary(delimiter: Option<char>, header: bool) -> String {
    let delim = match delimiter {
        Some('\t') => "\\t".to_string(),
        Some(c) => c.to_string(),
        None => "auto".to_string(),
    };
    let header = if header { "on" } else { "off" };
    format!("delim {delim} | header {header}")
}

#[cfg(test)]
#[path = "csv_opts_tests.rs"]
mod csv_opts_tests;
