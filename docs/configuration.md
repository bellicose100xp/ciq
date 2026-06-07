---
title: Configuration
layout: default
nav_order: 4
---

# Configuration

ciq reads an optional config file at `$XDG_CONFIG_HOME/ciq/config.toml`, falling back to
`$HOME/.config/ciq/config.toml`. The file is entirely optional — every section and every key has a
default, so an empty file (or no file at all) yields the conservative built-ins. A malformed config
never blocks startup; ciq falls back to defaults and surfaces a warning.

A CLI flag always wins over the corresponding config value (the precedence is
**CLI flag > config file > sniffed value**).

The file has five sections: `[general]`, `[theme]`, `[ai]`, `[history]`, and `[csv]`. Unknown
top-level tables are ignored, so a config written for a newer ciq still loads on an older binary.

```toml
[general]
row_limit    = 1000
threads      = 4
memory_limit = "4GB"

[theme]
mode = "auto"

[ai]
enabled     = false
provider    = "none"
model       = "claude-sonnet-4-5"
api_key_env = "ANTHROPIC_API_KEY"

[history]
enabled     = true
max_entries = 1000

[csv]
# delimiter / quote / header / type overrides (see below)
```

## `[general]`

Engine-wide defaults.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `row_limit` | integer | `1000` | The interactive viewport `LIMIT N` — how many rows a live query shows. `0` is clamped to `1`. |
| `threads` | integer | `4` | DuckDB worker-thread bound (`SET threads=<n>`). Unset applies ciq's bounded default (`4`) so a many-core host doesn't oversubscribe under rapid keystrokes. |
| `memory_limit` | string | DuckDB default | DuckDB memory cap as a size string (e.g. `"4GB"`, `"512MB"`), applied as `SET memory_limit='<s>'`. Unset leaves DuckDB's own default. A malformed value surfaces as a clean load error. |

## `[theme]`

Color polarity. ciq centralizes all colors internally; this section is the config surface a future
polarity pass reads.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `mode` | `auto` \| `light` \| `dark` | `auto` | Light/dark adaptation. `auto` lets the terminal decide; `light` / `dark` pin the polarity. |
| `overrides` | table of `string` -> `string` | empty | Forward-compatible per-surface color overrides (e.g. `"grid.header" = "Cyan"`). Unknown keys are ignored. |

## `[ai]`

Natural-language-to-SQL provider settings. Off by default. **No secret is ever stored here** — the
API key is named by an environment variable and read from the environment at call time.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `enabled` | bool | `false` | Master switch. Even with a provider set, `false` keeps the feature off. |
| `provider` | `none` \| `anthropic` | `none` | Which provider to call. `none` disables the feature regardless of `enabled`. |
| `model` | string | `claude-sonnet-4-5` | The model id. |
| `api_key_env` | string | `ANTHROPIC_API_KEY` | The name of the environment variable holding the API key (never the key itself). |

See [AI: natural language to SQL](features/ai-nl-to-sql.md) for how the feature is used.

## `[history]`

Query-history persistence. The in-session ring always works; this section controls only the
on-disk file.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `enabled` | bool | `true` | Persist history to disk. `false` keeps history session-only (the in-memory ring still works). |
| `max_entries` | integer | `1000` | Cap on entries kept (in-session ring and on-disk file). `0` is clamped to `1`. |
| `path` | string | XDG default | Explicit on-disk history file path. Unset uses the storage layer's XDG default. |

See [Query history](features/query-history.md) for how recall works.

## `[csv]`

CSV ingest dialect and type overrides — the persistent form of the ingest CLI flags. Every key here
is overridden by the matching CLI flag if you pass one. The keys mirror the flags documented in
[CSV ingest and overrides](features/csv-ingest.md): delimiter, quote, escape, header, null-string,
sample-size, per-column types, all-varchar, and date-format.

See the [Quick Reference](quick-reference.md#cli-flags) for the CLI form and precedence.
