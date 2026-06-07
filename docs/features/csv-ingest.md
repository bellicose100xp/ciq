---
title: CSV ingest and overrides
layout: default
parent: Features
nav_order: 8
---

# CSV ingest and overrides

ciq parses the CSV once at startup into the in-memory table `t`, using DuckDB's CSV reader. The
dialect (delimiter, quote, header) is auto-detected, and types are sniffed per column — but every
piece is overridable from the CLI or config when the auto-detection guesses wrong.

## Auto-detection

By default ciq sniffs the file's leading bytes for the delimiter (comma, semicolon, tab, or pipe;
ambiguous cases default to comma), the quote character, and whether the first row is a header.
DuckDB then does the thorough type-inference pass over the whole file, so columns come back typed
(`BIGINT`, `DOUBLE`, `DATE`, `VARCHAR`, ...) rather than all-string.

## Overriding the dialect and types

Every detail is overridable, and the precedence is **CLI flag > config file > sniffed value**:

| Flag | Effect |
|---|---|
| `--delim <CHAR>` | force the field delimiter (e.g. `;` or a tab) |
| `--quote <CHAR>` | force the quote character |
| `--escape <CHAR>` | set the escape character inside quoted fields |
| `--header` | treat the first row as a header |
| `--no-header` | treat the first row as data |
| `--null-string <STR>` | a string (e.g. `NA`) that ingests as SQL `NULL` |
| `--sample-size <N>` | rows the type sniffer samples (`-1` scans the whole file); alias `--sniff-rows` |
| `--types <SPEC>` | per-column type overrides, e.g. `zip=VARCHAR,amount=DECIMAL(12,2)` |
| `--all-varchar` | ingest every column as `VARCHAR` (no type sniffing) |
| `--date-format <FMT>` | explicit date parse format, e.g. `%d/%m/%Y` |

The same dialect/type keys can be set persistently in the [`[csv]`](../configuration.md#csv)
section of your config; a CLI flag always wins over config.

## Column names, ragged rows, and empties

- **Column names** are kept verbatim from the header and auto-quoted wherever they appear in
  generated SQL, so a column named `order` or `Total ($)` is safe. Duplicate or empty header names
  are de-duplicated by DuckDB (`id,name,,name` becomes `id,name,column2,name_1`).
- **Ragged rows** (rows with a different field count) degrade gracefully under auto-detection rather
  than panicking; an explicit `--delim` that does not fit yields a clean load error.
- **Empty fields** ingest as SQL `NULL` by default (the DuckDB default). Use `--null-string` to
  control which token becomes `NULL`.

See the [Configuration](../configuration.md) page for the config-file form and the
[Quick Reference](../quick-reference.md) for the flag list.
