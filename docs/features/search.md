---
title: Row search
layout: default
parent: Features
nav_order: 9
---

# Row search

Press **Ctrl+F** to filter the result grid down to the rows that contain what you type. The match
is case-insensitive and checked against every column's displayed text, so you don't need to know
which column holds the value — and every occurrence is highlighted in place so you can see *why*
each row matched.

This is a display-side filter over the current result: no SQL runs, the query in the bar is
untouched, and closing the search restores the full grid instantly. For a durable filter, write a
`WHERE` clause (the search needle is a good way to find what to filter on first).

## How to use it

- With a result on screen, press **Ctrl+F**. The search bar opens between the grid and the query
  box, and the grid filters live as you type. The first match is highlighted (in the current-match
  color) and scrolled into view as you type, before you confirm.
- The badge on the bar shows `shown/total` rows; it turns red when nothing matches.
- **Enter** (or **Tab**) confirms: the needle freezes, the bar dims, and the keyboard goes back to
  normal grid navigation (scrolling, `f` facets, `Ctrl+T`) over the filtered rows. The first
  matching row becomes the **current match**.
- Once confirmed, **n** / **N** (or **Enter**) step to the next / previous matching row. The
  current match is highlighted in a distinct color (bright, vs. the dim highlight on the other
  matches) and scrolled into view with a margin, so it's never pinned against a pane edge unless
  it's the very first or last row. Navigation wraps around the ends.
- On a confirmed search, **Ctrl+F** re-enters editing with the needle intact, and **Esc** clears
  the filter and closes the bar.
- While editing, **Esc** closes and clears; **Ctrl+C** quits from either mode.

## Semantics

- **Any column matches.** A row is kept when at least one cell's displayed text contains the
  needle, case-insensitively. Numbers match their digits (`2` matches `12` and `2.5`).
- **`NULL` never matches.** A SQL `NULL` is an absent value, not the text "NULL" — typing `null`
  finds cells whose *data* contains "null", never the dimmed placeholder glyph.
- **A new result re-applies the filter.** Editing the query while a search is confirmed filters
  the fresh rows with the standing needle.
- The row counter on the results border shows the filtered count; the search badge carries the
  `shown/total` arithmetic.
