---
title: Query history
layout: default
parent: Features
nav_order: 7
---

# Query history

Recall any query you have run before. History works in two scopes: an in-session ring that always
works, and an on-disk file that persists across runs.

## How to use it

- Press **Ctrl+R** to open the history popup. It seeds its filter with the current bar text, so the
  list pre-filters to similar prior queries.
- **Up / Down** move the selection through the (filtered, newest-first) list.
- Type to fuzzy-filter; **Backspace** removes a character from the filter.
- **Enter** or **Tab** recalls the selected query into the bar. The recalled SQL runs through the
  normal debounce -> preprocess (read-only guard) -> dispatch path, exactly like a typed query —
  recall is not a privileged bypass.
- **Esc** closes the popup. **Ctrl+C** quits.

## What gets recorded

A query is recorded the moment it is successfully dispatched (the "I ran this" moment). The ring is
newest-first and deduplicated, so re-running the same query bumps it to the top rather than adding
a duplicate. Blank queries are ignored.

## Persistence

By default history persists to a newline-delimited file under your XDG data directory. Control it
in the [`[history]`](../configuration.md#history) section:

```toml
[history]
enabled     = true        # false keeps history session-only (the in-memory ring still works)
max_entries = 1000        # caps both the in-session ring and the on-disk file
# path      = "..."       # overrides the default XDG location
```

With `enabled = false`, history is session-only — the in-memory ring still works, but nothing is
read from or written to disk.

## Mouse

Wheel over the popup moves the selection; a click on an entry recalls it into the bar and runs it;
a click outside the popup closes it without recalling. Hovering highlights the row under the
pointer.

See the [Quick Reference](../quick-reference.md) for the complete set.
