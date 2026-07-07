---
title: Live SQL grid
layout: default
parent: Features
nav_order: 1
---

# Live SQL grid

The headline feature. Type a DuckDB SQL query in the bar at the top of the screen and an aligned
result grid below updates within one debounce tick (150 ms after typing settles) — no "press enter
to run."

## How it works

The CSV is parsed once at startup into an in-memory columnar table named **`t`**. Every debounced
edit re-queries that resident table, so each interactive query is a 1-20 ms re-query rather than a
re-parse. A newer keystroke supersedes (and cancels) the query still running for the previous one,
so fast typing never falls behind.

## How to use it

- Launch with a file: `ciq data.csv`. The table is always `t`.
- Type any single read-only `SELECT` or `WITH ... SELECT` (CTE). The grid reflects it live:

  ```sql
  SELECT region, count(*) AS n FROM t GROUP BY 1 ORDER BY n DESC
  ```

- A trailing `;` is tolerated. Your own `ORDER BY` and `LIMIT` are honored.
- Multiple statements and any non-`SELECT` (INSERT / UPDATE / DELETE / DDL / PRAGMA) are rejected
  with a status-line message and never reach the engine — `t` can never be mutated by an
  interactive query.

## The viewport LIMIT

Interactive results are capped to a viewport budget (default 1000 rows, see
[`[general] row_limit`](../configuration.md#general)) so a `SELECT *` on a huge table returns a
screenful instantly instead of materializing everything. When ciq applies this cap and the result
fills it, a banner notes how many rows are shown and points you to `--output` to export the full
result. A query with your own `LIMIT` is never re-capped beyond your intent.

## Keys

| Context | Key | Action |
|---|---|---|
| Query bar | printable / Backspace / Delete | edit the query |
| Query bar | Left / Right / Home / End | move the cursor |
| Query bar | Down | move focus to the results grid |
| Results | Up / Down | scroll one row (Up at the top returns focus to the bar) |
| Results | PageUp / PageDown | scroll ten rows |
| Results | Left / Right | scroll one column horizontally |
| Results | Home | jump to the first row |
| Anywhere | Esc or Ctrl+C | quit |

## Mouse

Wheel over the grid scrolls it; a trackpad swipe scrolls horizontally. Hovering a row highlights it
with a background band and a bright left accent bar (in the pane's border color) that follows the
pointer.

See the [Quick Reference](../quick-reference.md) for the complete set.
