---
title: Output modes
layout: default
parent: Features
nav_order: 5
---

# Output modes

Run one query non-interactively and write its full result to stdout in a chosen format, then exit —
no TUI. This is the scripting path: pipe ciq into other tools, or export a slice of a CSV.

## How to use it

```sh
# Default query is `SELECT * FROM t` (the whole table):
ciq data.csv --output csv

# With an explicit query:
ciq data.csv --output json -q "SELECT region, count(*) FROM t GROUP BY 1 ORDER BY 2 DESC"
```

- `--output <FORMAT>` selects the format and switches to non-interactive mode.
- `-q` / `--query <SQL>` is the query to run; it defaults to `SELECT * FROM t`.

## Formats

| `--output` value | Result |
|---|---|
| `csv` | RFC 4180 CSV (fields quoted only when needed; embedded quotes doubled) |
| `tsv` | tab-separated, with tab / CR / LF / backslash escaped |
| `json` | an array of objects, one per row, with type fidelity |
| `markdown` | a GitHub-style table, numeric and temporal columns right-aligned |

## Null vs empty string

The output preserves the distinction between a SQL `NULL` and an empty string:

- In CSV, a `NULL` is an empty unquoted field.
- In JSON, a `NULL` is `null` and an empty string is `""`.

## No viewport cap

Unlike the interactive grid, the `--output` path is **not** capped to the viewport row limit — it
returns the full result. It still goes through the same read-only single-statement guard, so it can
never mutate the table.

## Copying the result to your clipboard

There is no in-session copy chord today. To put a result on your system clipboard, use the
`--output` path and pipe it to your OS clipboard tool:

```sh
# macOS:
ciq data.csv --output csv -q "SELECT * FROM t WHERE region = 'EU'" | pbcopy

# Linux (X11):
ciq data.csv --output tsv | xclip -selection clipboard

# Windows:
ciq data.csv --output csv | clip
```
