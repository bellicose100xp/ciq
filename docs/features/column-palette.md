---
title: Column palette
layout: default
parent: Features
nav_order: 3
---

# Column palette

A CSV-native shortcut for the most common operation — "show me these columns" — without
hand-writing a `SELECT` list. The palette is a popup over the known schema: pick columns, reorder
them, filter the list by name, and it generates the underlying SQL into the query bar for you.

## How to use it

- Press **Ctrl+K** to open the palette (it needs a loaded schema; it is a no-op while the file is
  still loading).
- The popup lists every column with a checkbox and its type badge.
- **Space** toggles the column under the cursor. Selected columns are projected in the order you
  check them.
- **Left / Right** reorder the cursor's checked column earlier or later in the projection.
- Type to filter the column list by name; **Backspace** removes a character from the filter.
- **Enter** emits the generated query into the bar — it then runs through the normal debounce ->
  worker path, exactly like a typed query — and closes the palette.
- **Esc** closes the palette without emitting.

## What it generates

The palette emits a canonical query, e.g. selecting `region` then `amount`:

```sql
SELECT region, amount FROM t LIMIT 1000
```

With no columns checked it emits `SELECT *`. Identifiers that need quoting are quoted
automatically. Any predicates the palette holds become a `WHERE` conjunction.

## Ownership and the Replace affordance

The palette only drives the bar while it "owns" the query — detected by a byte-compare of the bar
against the last query the palette emitted. The moment you hand-type SQL into the bar, the palette
no longer owns it and will not silently clobber your edits; instead it offers to replace the bar
with a freshly generated query. The common path (an empty bar at startup) begins palette-owned, so
the palette works out of the box without ever parsing your SQL.

See the [Quick Reference](../quick-reference.md) for the complete set.
