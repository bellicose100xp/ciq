---
title: AI - natural language to SQL
layout: default
parent: Features
nav_order: 6
---

# AI: natural language to SQL

Describe the query you want in plain language and have a model write the SQL. The prompt is
grounded on your table's live schema (every column and its type), and the model's reply runs
through the same read-only guard as a typed query — the AI can never smuggle a mutation past the
grid.

## How to use it

- Press **Ctrl+G** to open the AI popup. It is a no-op unless the AI feature is configured and a
  schema is loaded.
- Type a request in plain English, e.g. "top 5 regions by total amount".
- **Enter** submits the prompt. The model's SQL drops into the query bar and runs through the
  normal preprocess (read-only single-statement) guard, then the dispatcher and worker.
- **Esc** closes the popup. **Ctrl+C** quits. If a request errors, the popup stays open with your
  prompt preserved so you can edit and retry.

## Enabling it

The AI feature is off by default. Enable it in the [`[ai]`](../configuration.md#ai) section of your
config:

```toml
[ai]
enabled  = true
provider = "anthropic"
model    = "claude-sonnet-4-5"
api_key_env = "ANTHROPIC_API_KEY"
```

**No secret is ever stored in the config.** `api_key_env` names the environment variable that holds
the API key; the provider reads the key from the environment at call time. A config with
`provider = "none"` (the default) leaves the feature off.

## Safety

The generated SQL is not privileged. A `DROP TABLE`, a multi-statement reply, or any non-`SELECT`
is rejected by the same preprocess guard that vets a typed query and never reaches the engine.

See the [Quick Reference](../quick-reference.md) for the complete set.
