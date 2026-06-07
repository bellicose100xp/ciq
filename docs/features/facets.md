---
title: Instant facets
layout: default
parent: Features
nav_order: 4
---

# Instant facets

Surface the distribution of a column on demand. Because `DISTINCT` and `GROUP BY ... COUNT(*)` run
in single-digit-to-low-tens of milliseconds on the in-memory table, ciq can show a column's shape
instantly — something that would be unusably slow if it required re-parsing the file.

## How to use it

- Move focus to the results grid (press **Down** from the query bar).
- Press **f** to open a facet for the leftmost visible column.
- **Esc** closes the facet popup (it does not quit). **Ctrl+C** still quits. Any other key
  dismisses the popup and falls through to grid navigation.

A facet for a derived or expression column with no matching base-schema column is a no-op.

## What it shows

The facet adapts to the column's type:

- **Numeric / temporal / boolean columns** get a summary: min, max, distinct count, and null count.
- **Text and other columns** get a top-K histogram: the most frequent values with their counts and
  a proportional bar, plus the column-wide distinct and null counts.

The facet query rides the same worker channel as your grid query (no second connection) and never
touches the grid — opening a facet does not disturb your live result.

See the [Quick Reference](../quick-reference.md) for the complete set.
