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
row_limit    = 1000  # optional; omit for no cap (the default)
threads      = 4
memory_limit = "4GB"

[theme]
mode = "auto"

[ai]
enabled  = false
provider = "none"   # or "anthropic" / "bedrock"
effort   = "low"

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
| `row_limit` | integer | unset (no cap) | Optional interactive viewport `LIMIT N` — cap how many rows a live query shows. Unset or `0` means no cap: every row the query returns is shown (the default). |
| `threads` | integer | `4` | DuckDB worker-thread bound (`SET threads=<n>`). Unset applies ciq's bounded default (`4`) so a many-core host doesn't oversubscribe under rapid keystrokes. |
| `memory_limit` | string | DuckDB default | DuckDB memory cap as a size string (e.g. `"4GB"`, `"512MB"`), applied as `SET memory_limit='<s>'`. Unset leaves DuckDB's own default. A malformed value surfaces as a clean load error. |

## `[theme]`

Color polarity. ciq ships two palettes with the same hue families: the galaxy dark palette, and a
deeper light palette whose foregrounds hold at least 3:1 contrast against a white background. One
is chosen at startup and used for the whole session.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `mode` | `auto` \| `light` \| `dark` | `auto` | Which palette to use. `auto` asks the terminal for its background color (the OSC 11 query, then the `COLORFGBG` variable) and picks the palette whose polarity matches, falling back to dark when the terminal does not answer. `light` / `dark` skip the query and pin the palette. |
| `overrides` | table of `string` -> `string` | empty | Reserved per-surface color overrides (e.g. `"grid.header" = "Cyan"`). Accepted but not yet read. |

Most terminals answer the background query (iTerm2, Terminal.app, Ghostty, kitty, WezTerm,
Alacritty, GNOME Terminal, Windows Terminal is the exception). If yours does not, or you switch
your terminal theme with ciq open, pin the mode:

```toml
[theme]
mode = "light"
```

## `[ai]`

Provider and request settings for the AI chat (`Ctrl+A` to show or hide it, `Ctrl+G` to ask). Off by default. **No secret is ever stored
here**: the API key is named by an environment variable and
read from the environment at call time. This section rejects unknown keys: a typo like `api_key`
instead of `api_key_env` fails the parse and warns, rather than silently leaving a secret at rest in
a plaintext config while the tool reports everything is fine.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `enabled` | bool | `false` | Master switch. Even with a provider set, `false` keeps the chat off. |
| `provider` | `none` \| `anthropic` \| `bedrock` | `none` | Which provider to call. `anthropic` is the first-party Messages API at `api.anthropic.com`; `bedrock` is Claude in Amazon Bedrock, reached through `InvokeModel` on `bedrock-runtime.<region>.amazonaws.com` with the same request body. `none` disables the feature regardless of `enabled`. |
| `model` | string | `claude-fable-5-1`, or `global.anthropic.claude-fable-5-1` on Bedrock | The model id. Bedrock ids carry a routing prefix ahead of `anthropic.`: `global.` routes cross-region at no pricing premium, and `us.` / `eu.` / `jp.` / `apac.` pin an inference profile to one geography at a 10% premium. The first-party API takes the bare id. |
| `api_key_env` | string | `ANTHROPIC_API_KEY`, or `AWS_BEARER_TOKEN_BEDROCK` on Bedrock | The name of the environment variable holding the API key or Bedrock bearer token (never the key itself). On Bedrock, an unset variable falls back to SigV4 signing. |
| `region` | string | see below | Bedrock only. The AWS region in the endpoint host and in the SigV4 signature. Precedence: this key, `AWS_REGION`, `AWS_DEFAULT_REGION`, the profile's own `region`, then `us-east-1`. |
| `profile` | string | `AWS_PROFILE`, else `default` | Bedrock only. The named AWS profile SigV4 credentials are read from, including a profile whose credentials come from a `credential_process` command. |
| `effort` | `off` \| `low` \| `medium` \| `high` \| `xhigh` \| `max` | `low` | How hard the model thinks per request. `off` omits the effort and thinking fields entirely, for models that reject them. |
| `max_tokens` | integer | `4096` | Output token cap per request, enough for five suggestions with SQL and a note each. A configured `0` falls back to the default rather than asking for an empty reply. |
| `request_timeout_secs` | integer | `60` | Whole-request timeout in seconds. `0` disables the timeout. |
| `open_on_start` | bool | `true` | Show the chat as soon as the CSV loads. It opens without taking the keyboard, so typing still goes to the query bar until you click the chat or press `Ctrl+G`. A failing query pops the chat up even when this is `false`. |
| `extra_instructions` | string | unset | Free-text preferences appended to every prompt. It never replaces the built-in rules, and the output-format rules always win. |
| `max_context_length` | integer | `20000` | Character budget per request, spent on two things: the conversation turns sent with a message (the newest that fit; older ones are left out), and the result sample or error text embedded in that message (truncated when longer). A configured `0` falls back to the default. |

**Why `effort` defaults to `low`.** Thinking is always on for the Fable models, so effort is the
lever that decides how long and how expensively the model thinks before answering. Writing one SQL
suggestion against a schema you already handed it does not need deep reasoning, and a slow answer in
a live query loop is worse than a slightly less clever one. Raise it if you ask harder questions.

A full Bedrock config, the default path:

```toml
[ai]
enabled              = true
provider             = "bedrock"
model                = "global.anthropic.claude-fable-5-1"
region               = "us-east-1"
profile              = "default"
effort               = "low"
max_tokens           = 4096
request_timeout_secs = 60
max_context_length   = 20000
extra_instructions   = "Prefer CTEs over nested subqueries."
```

See [AI chat](features/ai-nl-to-sql.md) for how the chat is driven and how Bedrock credentials are
resolved.

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
