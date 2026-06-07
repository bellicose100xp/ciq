//! Static DuckDB candidate data — keywords, functions/aggregates, and operators (`dev/PLAN.md`
//! §5.4/§5.5, `dev/DECISIONS.md` S4).
//!
//! S4: **one combined file**, not a separate `duckdb_functions.rs`. Keywords, functions, and the
//! operator table are all static, position-filtered candidate data; one file is simpler and well
//! under the 1000-line split rule. This replaces jiq's `jq_functions.rs` builtins list — these are
//! DuckDB-dialect names, so a SQL tool offers `COUNT`/`strftime`, never jq's `to_entries`/`gsub`.
//!
//! Pure data, no behavior. Every table is **stable-ordered** (the determinism rule: anything
//! user-visible has a fixed order) — entries are listed in their canonical display order and the
//! candidate generator (P3.5) preserves it before fuzzy-ranking.
//!
//! This is distinct from `crate::sql_lexer`'s small `KEYWORDS` set, which only decides
//! `Keyword`-vs-`Ident` while lexing. The tables here are the richer *candidate* surface shown in
//! the popup (with type/agg labels, signatures, and descriptions).

/// A SQL function or aggregate offered in `SelectList` / `HAVING` positions. `signature` and
/// `description` feed the popup's right-aligned hint (jiq's `Suggestion::with_signature` /
/// `with_description` slots); `is_aggregate` selects the `Aggregate` vs `Function` suggestion kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FunctionEntry {
    /// The function name, canonical DuckDB casing for display (upper for aggregates, lower for
    /// scalar functions — matching how DuckDB docs present them).
    pub name: &'static str,
    /// A compact call signature, e.g. `COUNT(expr)`.
    pub signature: &'static str,
    /// One-line description for the popup hint.
    pub description: &'static str,
    /// Whether this is an aggregate (legal only in `SelectList`/`HAVING`) vs a scalar function.
    pub is_aggregate: bool,
}

/// A comparison/membership operator offered in a `ComparisonOp` position. `text` is what gets
/// inserted; `label` is a short popup hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperatorEntry {
    /// The operator text inserted into the query (e.g. `=`, `LIKE`, `IS NOT NULL`).
    pub text: &'static str,
    /// A short human label for the popup.
    pub label: &'static str,
}

/// DuckDB SQL clause keywords offered in a bare [`crate::autocomplete::clause_context::CursorContext::Keyword`]
/// position. Stable order: the common clause-builder sequence first, then modifiers. These are
/// upper-cased for display (the conventional SQL keyword casing).
pub const KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "GROUP BY",
    "HAVING",
    "ORDER BY",
    "LIMIT",
    "OFFSET",
    "DISTINCT",
    "AS",
    "JOIN",
    "INNER JOIN",
    "LEFT JOIN",
    "RIGHT JOIN",
    "FULL JOIN",
    "CROSS JOIN",
    "ON",
    "USING",
    "WITH",
    "UNION",
    "UNION ALL",
    "EXCEPT",
    "INTERSECT",
    "AND",
    "OR",
    "NOT",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "ASC",
    "DESC",
];

/// Aggregate functions (`is_aggregate = true`) followed by common DuckDB scalar functions. Stable
/// order: the five canonical aggregates first (the §5.4 `COUNT/SUM/AVG/MIN/MAX` row), then scalars.
pub const FUNCTIONS: &[FunctionEntry] = &[
    FunctionEntry {
        name: "COUNT",
        signature: "COUNT(expr)",
        description: "count of non-null rows",
        is_aggregate: true,
    },
    FunctionEntry {
        name: "SUM",
        signature: "SUM(expr)",
        description: "sum of values",
        is_aggregate: true,
    },
    FunctionEntry {
        name: "AVG",
        signature: "AVG(expr)",
        description: "mean of values",
        is_aggregate: true,
    },
    FunctionEntry {
        name: "MIN",
        signature: "MIN(expr)",
        description: "minimum value",
        is_aggregate: true,
    },
    FunctionEntry {
        name: "MAX",
        signature: "MAX(expr)",
        description: "maximum value",
        is_aggregate: true,
    },
    // Common scalar functions (DuckDB dialect). Lower-cased per DuckDB docs convention.
    FunctionEntry {
        name: "lower",
        signature: "lower(s)",
        description: "lowercase a string",
        is_aggregate: false,
    },
    FunctionEntry {
        name: "upper",
        signature: "upper(s)",
        description: "uppercase a string",
        is_aggregate: false,
    },
    FunctionEntry {
        name: "length",
        signature: "length(s)",
        description: "character length",
        is_aggregate: false,
    },
    FunctionEntry {
        name: "trim",
        signature: "trim(s)",
        description: "strip surrounding whitespace",
        is_aggregate: false,
    },
    FunctionEntry {
        name: "round",
        signature: "round(x, n)",
        description: "round to n decimals",
        is_aggregate: false,
    },
    FunctionEntry {
        name: "coalesce",
        signature: "coalesce(a, b, ...)",
        description: "first non-null argument",
        is_aggregate: false,
    },
    FunctionEntry {
        name: "cast",
        signature: "cast(x AS type)",
        description: "convert to a type",
        is_aggregate: false,
    },
    FunctionEntry {
        name: "date_trunc",
        signature: "date_trunc(part, ts)",
        description: "truncate a timestamp to a part",
        is_aggregate: false,
    },
    FunctionEntry {
        name: "strftime",
        signature: "strftime(ts, fmt)",
        description: "format a timestamp",
        is_aggregate: false,
    },
];

/// Comparison and membership operators offered in a `ComparisonOp` position (the §5.4 operator
/// row). Stable order: the comparison operators, then the membership/null tests. `IS NULL` /
/// `IS NOT NULL` are written as full phrases because that is what gets inserted.
pub const OPERATORS: &[OperatorEntry] = &[
    OperatorEntry {
        text: "=",
        label: "equals",
    },
    OperatorEntry {
        text: "!=",
        label: "not equals",
    },
    OperatorEntry {
        text: "<",
        label: "less than",
    },
    OperatorEntry {
        text: "<=",
        label: "less than or equal",
    },
    OperatorEntry {
        text: ">",
        label: "greater than",
    },
    OperatorEntry {
        text: ">=",
        label: "greater than or equal",
    },
    OperatorEntry {
        text: "LIKE",
        label: "pattern match",
    },
    OperatorEntry {
        text: "IN",
        label: "in a value list",
    },
    OperatorEntry {
        text: "BETWEEN",
        label: "within a range",
    },
    OperatorEntry {
        text: "IS NULL",
        label: "is null",
    },
    OperatorEntry {
        text: "IS NOT NULL",
        label: "is not null",
    },
];

#[cfg(test)]
#[path = "sql_keywords_tests.rs"]
mod sql_keywords_tests;
