---
title: Output modes
layout: default
parent: Features
nav_order: 5
---

# Output modes

Get a result out of ciq two ways: **non-interactively** with `--output` (the scripting path), or
**from inside the session** with the `Ctrl+O` / `Ctrl+W` exit chords (take the grid you're looking
at with you).

## In-session: Ctrl+O and Ctrl+W

While you have a result on screen, two chords deliver it. Both act on the **displayed** rows, so a
`Ctrl+F` filter narrows what gets output:

- **`Ctrl+O`** quits and prints the result to the terminal as an aligned, colored table (headers in
  cyan, `NULL`s dimmed, numbers right-aligned), so it lands in your scrollback. This is ciq's take
  on `jiq`'s output-on-exit. `Enter` is deliberately *not* the output key — it already means
  newline / confirm / accept elsewhere, so overloading it would be ambiguous.
- **`Ctrl+W`** opens a save popup: type a filename and press `Enter` to write the result as
  RFC-4180 CSV. A leading `~/` expands to your home directory, and a name with no extension gains
  `.csv` (the output is CSV, so the extension should say so). The popup previews the resolved path
  and warns before overwriting an existing file. The default filename is `<source>-out.csv`.

## Non-interactive: --output

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

The `--output` path always returns the full result — even when the interactive grid has a
configured `[general] row_limit` cap, `--output` ignores it. It still goes through the same
read-only single-statement guard, so it can never mutate the table.

## Copying the result to your clipboard

There is no in-session clipboard chord today (use `Ctrl+W` to save to a file, or `Ctrl+O` to print
to the scrollback). To put a result directly on your system clipboard, use the `--output` path and
pipe it to your OS clipboard tool:

```sh
# macOS:
ciq data.csv --output csv -q "SELECT * FROM t WHERE region = 'EU'" | pbcopy

# Linux (X11):
ciq data.csv --output tsv | xclip -selection clipboard

# Windows:
ciq data.csv --output csv | clip
```
