---
title: Quick Reference
layout: default
nav_order: 3
---

# Quick Reference

Every keyboard shortcut and every CLI flag in one place. For what each capability does, see the
[Features](features/) pages.

## CLI flags

Usage: `ciq [OPTIONS] [FILE]`

| Flag | Argument | Meaning |
|---|---|---|
| `FILE` | (positional) | CSV file to open. Omitting it is reserved for stdin ingest (a later phase). |
| `--debug` | | Enable debug logging to `/tmp/ciq/ciq-debug.log` (file only; never the terminal). |
| `--delim` | `<CHAR>` | Field delimiter (e.g. `;` or a tab). Overrides sniffing. |
| `--quote` | `<CHAR>` | Quote character. Overrides sniffing. |
| `--escape` | `<CHAR>` | Escape character inside quoted fields. |
| `--header` | | Treat the first row as a header (inverse of `--no-header`). |
| `--no-header` | | Treat the first row as data, not a header. |
| `--null-string` | `<STR>` | String that ingests as SQL `NULL` (e.g. `NA`). |
| `--sample-size` | `<N>` | Rows the type sniffer samples (`-1` scans the whole file). Alias: `--sniff-rows`. |
| `--types` | `<SPEC>` | Per-column type overrides, e.g. `zip=VARCHAR,amount=DECIMAL(12,2)`. |
| `--all-varchar` | | Ingest every column as `VARCHAR` (no type sniffing). |
| `--date-format` | `<FMT>` | Explicit date parse format, e.g. `%d/%m/%Y`. |
| `--output` | `<FORMAT>` | Run a query non-interactively, print the result in this format, then exit (no TUI). One of `csv`, `tsv`, `json`, `markdown`. |
| `-q`, `--query` | `<SQL>` | The SQL to run on the `--output` path. Defaults to `SELECT * FROM t`. |
| `-h`, `--help` | | Print help. |
| `-V`, `--version` | | Print version. |

## Keyboard shortcuts

The table is always named `t`. Inside the session:

### Global

| Key | Action |
|---|---|
| `Ctrl+K` | Open the column palette (needs a loaded schema). |
| `Ctrl+R` | Open the query-history popup (seeds its filter with the bar text). |
| `Ctrl+G` | Open the AI natural-language popup (needs the AI feature configured + a schema). |
| `Ctrl+F` | Open the row-filter search bar (needs a result on screen); on a confirmed search, re-enter editing. |
| `Esc` | Quit (when no popup is open; closes the focused popup otherwise). |
| `Ctrl+C` | Quit (from anywhere, including any open popup). |

### Query bar (focus: query bar)

| Key | Action |
|---|---|
| printable character | Insert the character. |
| `Backspace` | Delete the character before the cursor. |
| `Delete` | Delete the character at the cursor. |
| `Left` / `Right` | Move the cursor one character. |
| `Home` / `End` | Move the cursor to the start / end of the query. |
| (paste) | Insert a bracketed-paste payload at the cursor. |
| `Down` | Move focus to the results grid. |

### Results grid (focus: results)

| Key | Action |
|---|---|
| `Up` | Scroll up one row; at the top, return focus to the query bar. |
| `Down` | Scroll down one row. |
| `PageUp` / `PageDown` | Scroll ten rows up / down. |
| `Left` / `Right` | Scroll one column left / right. |
| `Home` | Jump to the first row. |
| `f` | Open a facet for the leftmost visible column. |
| `Esc` | Clear a confirmed search filter (restores the full grid). |

### Search bar (Ctrl+F, while editing)

| Key | Action |
|---|---|
| printable character | Append to the filter needle (the grid filters live). |
| `Backspace` | Delete the last needle character. |
| `Enter` / `Tab` | Confirm: freeze the filter, resume grid navigation. |
| `Esc` | Close the bar and clear the filter. |
| `Ctrl+C` | Quit. |

### Autocomplete popup (when open)

| Key | Action |
|---|---|
| `Up` / `Down` | Move the selection (wraps). |
| `Tab` / `Enter` | Accept the selected candidate and dismiss. |
| `Esc` | Dismiss the popup (does not quit). |

### Column palette (Ctrl+K)

| Key | Action |
|---|---|
| `Space` | Toggle the column under the cursor (selection order drives projection order). |
| `Up` / `Down` | Move the cursor through the filtered list. |
| `Left` / `Right` | Reorder the cursor's checked column earlier / later. |
| printable character | Append to the fuzzy filter. |
| `Backspace` | Remove a character from the filter. |
| `Enter` | Emit the generated query into the bar and close. |
| `Esc` | Close without emitting. |

### Query-history popup (Ctrl+R)

| Key | Action |
|---|---|
| `Up` / `Down` | Move the selection through the filtered, newest-first list. |
| `Enter` / `Tab` | Recall the selected query into the bar and close. |
| printable character | Fuzzy-filter the list. |
| `Backspace` | Remove a character from the filter. |
| `Esc` | Close the popup. |

### AI popup (Ctrl+G)

| Key | Action |
|---|---|
| printable character | Append to the prompt. |
| `Backspace` | Delete the character before the cursor. |
| `Enter` | Submit the prompt to the model. |
| `Esc` | Close the popup. |

## Mouse

Mouse capture is on for the whole session (native terminal text-selection is replaced by ciq's own
interactions; use the output modes to copy data out).

| Gesture | Where | Action |
|---|---|---|
| Wheel up / down | results grid (or anywhere outside a popup) | Scroll the grid three rows per notch. |
| Wheel up / down | open list popup (autocomplete / palette / history) | Move that popup's selection. |
| Trackpad swipe left / right | results grid | Smooth character-granular horizontal scroll. |
| Click | results grid | Focus the grid. |
| Click | a query pane / the query bar | Focus it and place the cursor at the clicked character (Insert mode). |
| Click | autocomplete row | Select that candidate (Tab / Enter accepts). |
| Double-click | autocomplete row | Accept that candidate. |
| Click | palette row | Move the cursor onto that column. |
| Double-click | palette row | Toggle that column (like Space). |
| Click | history row | Recall that query into the bar and run it. |
| Click | outside an open facet / history / AI popup | Dismiss the popup. |
| Hover | grid row or popup row | Highlight the row under the pointer (grid rows also get a bright left accent bar that follows the pointer). |
