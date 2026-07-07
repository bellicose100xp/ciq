# Changelog

All notable changes to ciq are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and ciq uses
[pre-1.0 versioning](CLAUDE.md#versioning--releasing): minor `0.X.0` for features and breaking
changes alike, patch `0.minor.Y` for fixes, refactors, polish, and docs.

## [Unreleased]

### Added
- **Row search** (`Ctrl+F`) — filter the grid to rows where any column contains the typed text
  (case-insensitive), with every match highlighted in place. Enter confirms the filter and
  returns the keyboard to grid navigation; Esc clears it. The facet chord is now strictly the
  modifier-free `f` (it previously also fired on `Ctrl+F`). Once confirmed, `n` / `N` (or Enter)
  step between matching rows: the current match gets a distinct highlight color and is scrolled
  into view with a scrolloff margin (vim-style, both axes), and highlights persist while
  scrolling. The first match is auto-highlighted and scrolled to live while you type (before
  confirming), matching jiq. Matches are scoped per cell so a needle never spans the column
  gutter, and the match-highlight band is brighter for legibility.

### Changed
- **No row cap by default** — interactive queries now show every row they return; the previous
  implicit `LIMIT 1000` viewport wrap is gone. Capping is a user choice: set
  `[general] row_limit` in the config (or type a `LIMIT`) to opt back in; `0`/unset means
  uncapped. The Simple-mode LIMIT pane starts empty accordingly.

## [0.1.0] - 2026-07-07

First tagged release. ciq is a terminal tool for querying a CSV with live DuckDB SQL: type a query,
watch an aligned grid update as you type, against an in-memory columnar table parsed once at
startup.

### Added
- **Live SQL grid** — debounced, interrupt-on-restart query loop against an embedded, bundled
  DuckDB engine; the grid repaints as you type, and a pipeline error keeps the last good result
  dimmed rather than clearing it.
- **Simple and Power query modes** — a five-pane clause form (SELECT / WHERE / GROUP BY / ORDER BY
  / LIMIT) or a free-form SQL textarea, toggled with `Ctrl+Q`.
- **Schema-aware autocomplete** — columns with type hints, distinct-value suggestions in
  comparisons, SQL keywords, and per-pane context in Simple mode.
- **Column palette** (`Ctrl+P`) — pick and reorder columns without hand-writing a SELECT list,
  live-rewriting the SELECT pane.
- **Instant facets** (`f`) — an on-demand distribution summary or top-K histogram for the focused
  column.
- **Query history** (`Ctrl+R`) — a searchable, on-disk ring of prior queries with fuzzy recall.
- **AI natural-language-to-SQL** (`Ctrl+G`) — describe a query in plain English against the live
  schema (requires a configured provider).
- **Output modes** — dump a result to CSV, TSV, JSON, or Markdown.
- **Mouse support** — wheel and trackpad scrolling, click-to-focus, click-to-position the cursor,
  popup row selection, double-click to accept an autocomplete suggestion or toggle a palette
  column, single-click history recall, click-outside-to-dismiss for the facet/history/AI popups,
  and a hover highlight with a bright left accent bar that follows the pointer across grid rows.
- **Release flow** — cargo-dist shell installer (curl-based) for macOS and Linux, built by the
  `Release` workflow on a `vX.Y.Z` tag.

[Unreleased]: https://github.com/bellicose100xp/ciq/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/bellicose100xp/ciq/releases/tag/v0.1.0
