# Changelog

All notable changes to ciq are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and ciq uses
[pre-1.0 versioning](CLAUDE.md#versioning--releasing): minor `0.X.0` for features and breaking
changes alike, patch `0.minor.Y` for fixes, refactors, polish, and docs.

## [Unreleased]

## [0.2.1] - 2026-07-10

### Fixed
- **Windows build** — the release build for `x86_64-pc-windows-msvc` failed to link because
  bundled DuckDB calls the Windows Restart Manager (`RmStartSession` and friends) without
  `rstrtmgr.lib` on the link line. A build script now links it on Windows, so the Windows target
  builds and ships. No effect on macOS or Linux.

## [0.2.0] - 2026-07-10

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
- **Save to CSV** (`Ctrl+W`) — write the result currently on screen (filtered rows included) to a
  file. A popup takes the filename, seeds it from the source CSV's stem (`<stem>-out.csv`),
  expands `~`, and previews the resolved path; a write error surfaces inline instead of crashing.
- **Console output on exit** — quitting with `Ctrl+O` prints the displayed grid to the scrollback
  as an aligned, colored table (headers, dimmed NULLs, right-aligned numerics, a row-count
  footer), so a result survives after the TUI closes.

### Changed
- **No row cap by default** — interactive queries now show every row they return; the previous
  implicit `LIMIT 1000` viewport wrap is gone. Capping is a user choice: set
  `[general] row_limit` in the config (or type a `LIMIT`) to opt back in; `0`/unset means
  uncapped. The Simple-mode LIMIT pane starts empty accordingly.
- **Modern popup styling** — every popup (autocomplete, column palette, history, facet, AI) now
  paints an opaque background so the grid no longer shows through it. The selected row uses a
  solid accent band with a bright left accent bar (`▌`) instead of reverse-video, and every row
  reserves a matching left gutter so contents stay column-aligned.

### Fixed
- **Typing and search stay fluid on large results** — with the row cap gone, a redraw used to
  re-format the whole result table on every keystroke, so a million-row file lagged badly per
  key. Grid layout now formats only the on-screen page (~0.3 ms per frame regardless of row
  count), and the search filter uses a no-allocation ASCII scan on the common case, cutting a
  1M-row filter from roughly 7 s to under 0.6 s.

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

[Unreleased]: https://github.com/bellicose100xp/ciq/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/bellicose100xp/ciq/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/bellicose100xp/ciq/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/bellicose100xp/ciq/releases/tag/v0.1.0
