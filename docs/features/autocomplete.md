---
title: Schema-aware autocomplete
layout: default
parent: Features
nav_order: 2
---

# Schema-aware autocomplete

Because the table is parsed once and its schema is known, completion is typed and grounded in the
actual file. As you type, a popup offers candidates appropriate to where the cursor is in the
query.

## What it suggests

The candidate source depends on the SQL context the cursor is in:

| Cursor context | Suggestions |
|---|---|
| `SELECT` list | column names (with type hints), `*`, and aggregate / scalar functions |
| `FROM` | the single table relation `t` |
| `WHERE` / `HAVING` predicate | column names (aggregates are withheld — illegal in a bare `WHERE`) |
| after a column in a predicate | comparison operators (`=`, `!=`, `<`, `<=`, `>`, `>=`, `LIKE`, `IN`, `BETWEEN`, `IS NULL`, `IS NOT NULL`) |
| a value position (e.g. `WHERE region = `) | the distinct actual values in that column |
| `GROUP BY` / `ORDER BY` | column names |
| elsewhere | DuckDB clause keywords |

Type hints in the popup reflect the column's sniffed DuckDB type (`int`, `float`, `date`, etc.), so
you can see at a glance what a column holds.

## Value completion

When the cursor is in a value position, ciq completes against the **distinct actual values** in
that column. The distinct values are fetched lazily, once per column, through the same worker that
runs your grid query (no second database connection) — the popup fills in once the values arrive.

## Insertion and quoting

Selecting a candidate inserts it with the right SQL quoting:

- A column whose name collides with a keyword (e.g. `order`) or is not a bare identifier is
  double-quoted: `"order"`.
- A text or temporal value becomes a single-quoted literal with embedded quotes doubled:
  `'O''Brien'`. Numeric values are inserted bare.

## Keys

| Key | Action |
|---|---|
| (typing) | the popup opens and re-ranks as you type |
| Up / Down | move the selection (wraps) |
| Tab or Enter | accept the selected candidate and dismiss |
| Esc | dismiss the popup (does not quit) |

See the [Quick Reference](../quick-reference.md) for the complete set.
