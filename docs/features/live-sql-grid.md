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

## Column colors

Each column is painted in its own soft pastel hue, rotating through an eight-color palette, so
adjacent columns read as distinct bands when you scan across a wide result. The header label shares
the hue of the data below it, and a column keeps its color as you scroll horizontally. A SQL `NULL`
still renders dim (an absent value reads as absent) and a search match still takes the highlight
band — both sit on top of the column color. The `Ctrl+O` console dump uses the same palette, so the
printed table matches what was on screen.

## The optional viewport LIMIT

By default there is no row cap — a query shows every row it returns, and how many rows come back
is your choice (write a `LIMIT`, or don't). If you want a standing cap, set
[`[general] row_limit`](../configuration.md#general); then a query with no `LIMIT` of its own is
wrapped to that budget, and when the result fills it a row counter notes the cap and `--output`
exports the full result. A query with your own `LIMIT` is never re-capped beyond your intent.

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
