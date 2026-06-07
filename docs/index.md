---
title: Home
layout: default
nav_order: 1
---

# ciq

**CSV Interactive Query** — type a DuckDB SQL query in a bar and watch an aligned result grid
update live as you type, against an in-memory columnar table parsed once at startup. It is to a
CSV what `jiq` is to JSON: no edit / run / inspect cycle, just a live query loop.

## The pitch: parse once, query many

Most CSV tools re-read the file for every operation. ciq does the opposite. On startup it parses
the CSV **exactly once** into a resident, in-memory columnar table (embedded DuckDB). Every
keystroke after that re-queries that already-parsed table, so interactive queries land in
single-digit-to-low-tens of milliseconds even on multi-million-row files — far under the 150 ms
debounce window. You see the effect of every clause one debounce tick after you type it.

What you get on top of live SQL:

- **Schema-aware autocomplete** — column names, their sniffed types, distinct cell values, and
  DuckDB keywords/functions, all grounded in the actual file.
- **Column palette** — pick, reorder, and filter columns without hand-writing a `SELECT` list.
- **Instant facets** — the distribution (top values + counts, or min/max/distinct) of any column
  on demand.
- **AI NL to SQL** — describe what you want in plain language; the model is grounded on your
  table's schema and its reply runs through the same read-only guard as a typed query.
- **Query history** — recall any prior query, in-session and persisted across runs.
- **Output modes** — dump a query result to CSV, TSV, JSON, or Markdown for scripting.

## Install

ciq ships as a single self-contained binary — DuckDB is statically compiled in, so there is
nothing else to install at runtime (no `jq`-style external binary, no DuckDB install, no shared
libraries).

```sh
# From source (needs a Rust toolchain and a C++ compiler at build time only):
cargo install ciq
```

> The first build is slow (~90 s to 2 min) because it compiles DuckDB's C++ amalgamation.
> Incremental rebuilds are fast. A downloaded release binary needs none of this.

## Run

```sh
# Open a file in the interactive TUI:
ciq data.csv

# Run one query and print the result, no TUI:
ciq data.csv --output csv -q "SELECT region, count(*) FROM t GROUP BY 1 ORDER BY 2 DESC"
```

Inside the session the table is always named `t`. Type any read-only `SELECT` (or `WITH ... SELECT`)
and the grid updates live. See the [Quick Reference](quick-reference.md) for every key and flag, or
the [Features](features/) pages for what each capability does and how to drive it.

## Learn more

- [Quick Reference](quick-reference.md) — every keyboard shortcut and CLI flag in one table.
- [Features](features/) — one page per capability.
- [Configuration](configuration.md) — every config key in `~/.config/ciq/config.toml`.

ciq is the standalone sibling of [`jiq`](https://github.com/fiatjaf/jiq) and the live spec lives in
its repository's `dev/PLAN.md`.
