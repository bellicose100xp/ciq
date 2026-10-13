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

By default ciq hands the file to DuckDB's CSV sniffer with no hints at all. DuckDB picks the
delimiter (comma, semicolon, tab, pipe, and more), the quote and escape characters, and whether the
first row is a header by trying every combination against a sample of the file and keeping the one
that parses most consistently. It then does the type-inference pass over the whole file, so columns
come back typed (`BIGINT`, `DOUBLE`, `DATE`, `VARCHAR`, ...) rather than all-string.

ciq does no sniffing of its own. A guess made from the first few lines and passed to DuckDB as a
fixed delimiter would only shrink its search space, and a wrong guess turns a file DuckDB reads fine
into a load error. The file extension plays no part either: a `.tsv` is read by its contents.

Two cases DuckDB cannot settle from the data, and where the flags below are the answer:

- **An all-text file** gives no type contrast between the first row and the rest, so DuckDB has to
  guess whether that row is a header. `--header` or `--no-header` settles it.
- **A file where two delimiters split every line equally well** (say, exactly one comma and one tab
  per line) is a genuine tie. `--delim` settles it.

## Overriding the dialect and types

Every detail is overridable, and the precedence is **CLI flag > config file > DuckDB's detection**:

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
