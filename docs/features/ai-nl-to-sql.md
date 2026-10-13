---
title: AI chat
layout: default
parent: Features
nav_order: 6
---

# AI chat

One popup sits on top of the query loop: `Ctrl+A` shows or hides it, `Ctrl+G` puts the keyboard in it.
You ask a question in plain language,
the model answers with a line or two of prose plus any queries worth running, and you pick one or
keep asking. Each follow-up is sent with the prior turns attached, so "now only the EU region, and
sort descending" refines the answer you just got.

Nothing the model writes is privileged. Every piece of SQL it produces crosses the same read-only
single-statement guard as a query you type, so a `DROP TABLE` or a multi-statement reply never
reaches the engine.



## What the box shows

The top border names the provider and model. The bottom border is the shortcut legend, and it
changes with focus: `Enter send · Alt+1-5 apply · Alt+↑↓ select · Ctrl+L new · Esc back · Ctrl+A close`
while the chat has the keyboard, `Ctrl+G ask · Alt+1-5 apply · Ctrl+A close` while it does not. The
input line shows a block cursor whenever the chat has the keyboard. Prose wraps to the
box width. Each suggestion is a block: `[n] fix` or `[n] optimize`, the SQL wrapped under a hanging
indent so long queries stay readable, and the model's one-line note dimmed beneath it. A blank line
separates one exchange from the next. The cursor band covers the whole block, and a click anywhere
in a block applies it.

## Picking a suggestion keeps your mode

Applying a suggestion respects the query mode you are in. In the five-pane Simple mode the SQL is
split into the panes through the same simplifier `Ctrl+Q` uses, so `WHERE`, `GROUP BY`, `ORDER BY`
and `LIMIT` land where you expect and you stay in the panes. Only a query the panes cannot express
(a CTE, a join, a subquery, `HAVING`) switches you to SQL mode, and the status line says so. In SQL
mode the query replaces the editor text.

## Where it sits, and who has the keyboard

The chat opens at the bottom right of the results pane, just above the query box, taking about
70% of the pane's width and 40% of its height (more on a taller terminal). By default it is open
as soon as the CSV loads (`[ai] open_on_start`), but it opens without the keyboard: typing goes
to the query bar as usual, and the chat sits visible with a muted border, ready to comment on a
failing query.

Focus follows your clicks. Click the chat's input line and the border brightens: typing now
writes your question. Click the query bar and the cursor lands where you clicked, with the chat
still visible. Click the grid to focus it. On the keyboard, `Ctrl+G` moves between the bar and the
chat (and `Esc` from the chat goes back to the bar), while `Ctrl+A` only ever shows or hides the box.
While the chat is visible but unfocused, `Alt+1` through `Alt+5` still apply the latest answer's
rows, so you can pick a suggestion without leaving the bar.

A failing query speaks up only while the chat is visible. It adds the turn without taking the
keyboard, so the fixes are on screen while you are still editing. A chat you hid with `Ctrl+A`
stays hidden and sends nothing until you bring it back. A horizontal rule separates each exchange
in the transcript.

## Opening it

- Press **Ctrl+A** to show the chat with the keyboard in it; pressing it again hides it. **Ctrl+G**
  puts the keyboard in a visible chat, or opens a hidden one. Both are no-ops unless the AI feature
  is configured and a schema is loaded.
- The popup opens above the query bar: a scrollable transcript, and a `> ` prompt on the bottom row.
  Each turn carries a `you:` or `ai:` label.
- The transcript **survives closing the popup**. Reopen with `Ctrl+A` and the conversation resumes
  where you left it. `Ctrl+L` starts a new one.

## Asking

Type a question and press `Enter`. The reply is prose plus zero or more numbered suggestions:

```
you:  top 5 regions by total amount
ai:   Group by region, sum the amount, take the top five.
 [1] optimize SELECT region, sum(amount) AS total FROM t GROUP BY 1 ORDER BY 2 DESC LIMIT 5   the ranking you asked for
```

Ask a follow-up and it goes out with the conversation behind it, so the model can build on what it
already proposed rather than starting over. "Now only the EU region, and sort ascending" produces the
full new query, not a fragment.

A question can be answered with prose alone. "What does the status column probably contain?" gets an
explanation and an empty suggestion list, which is a normal answer, not a failure.

Each turn occupies one transcript row, so a long answer is truncated with an ellipsis. While a
request is in flight the prompt row reads `thinking…` and typing is ignored, so one `Enter` never
fires two overlapping requests. A failed request shows an `error: …` line and leaves the transcript
intact, so you can ask again without losing the thread.

## Keys inside the chat

| Key | Action |
|---|---|
| printable character | Append to the line being typed. |
| `Backspace` | Delete the last character. |
| `Enter` | Send the typed line. With the line blank and the cursor on a suggestion, apply that suggestion instead. |
| `Alt+1` … `Alt+5` | Apply that numbered row of the most recent answer. |
| `Alt+Up` / `Alt+Down` | Move the cursor over every suggestion in the transcript (`Alt+K` / `Alt+J` too). |
| `Up` / `Down` | Scroll the transcript one row. |
| `PageUp` / `PageDown` | Scroll the transcript five rows. |
| `Ctrl+L` | Start a new conversation (drops every turn). |
| `Esc` / `Ctrl+G` | Hand the keyboard back to the query bar, leaving the chat visible. |
| `Ctrl+A` | Hide the popup, keeping the transcript. |
| `Ctrl+C` | Quit. |

The wheel scrolls the transcript, clicking a suggestion row applies that query the way `Alt+<n>` does, clicking the input line gives the chat the keyboard, and a click outside the popup hands the keyboard back without closing it.

The `Alt+<n>` digits always mean the answer you are looking at, the most recent one. The
`Alt+Up` / `Alt+Down` cursor walks every suggestion in the whole transcript, so you can scroll back
to an earlier answer and apply a row from it with `Enter`.

## Applying a suggestion

Picking a row drops its SQL into the query bar and runs it through the normal path: the read-only
single-statement preprocess guard, then the dispatcher and the worker, exactly as a typed query.
Generated SQL always lands you in Power mode so you can refine it before the next dispatch, and the
mode flip says so in the status line.

The popup **stays open**, so the conversation continues against the new result.

## When a query fails

A failing query appends a turn to the transcript reporting the error and asking for fixes. This is
the only message ciq sends without you typing, and it happens only while the chat is already open,
never while another request is in flight. A *successful* query triggers nothing: there is no
per-result refresh.

## Suggestion kinds

There are two, and rows are colored by kind:

| Kind | Meaning | Color |
|---|---|---|
| `fix` | Repairs the query that failed. | The error hue. |
| `optimize` | A faster or cleaner equivalent of the current query, or the query you asked for. | The success hue. |

Turn labels are colored too: `you:` in cyan, `ai:` in the popup's purple.

Every suggestion passes through the read-only single-`SELECT` guard *before* it is drawn, so an
unsafe or multi-statement suggestion never appears in the transcript at all.

### What the model is told

Each request carries three parts:

- The **system prompt**: the schema (every column and its sniffed type) and the reply contract. It
  does not change while a conversation runs, so it stays a cacheable prefix.
- The **newest transcript turns** that fit the `max_context_length` character budget, oldest first,
  always starting on one of your turns. Older turns fall off the front of what is sent; the full
  transcript stays on screen.
- The **new message**: your question, the query currently in the bar, and what that query last did
  (the engine's error message, an excerpt of up to ten result rows, or `0 rows`). The error text and
  the row excerpt are truncated to the same budget.

## Enabling it

The AI feature is off by default. Turn it on in the [`[ai]`](../configuration.md#ai) section of your
config. Two providers are available, both speaking the Anthropic Messages API.

The first-party Anthropic API:

```toml
[ai]
enabled     = true
provider    = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
```

Claude in Amazon Bedrock:

```toml
[ai]
enabled  = true
provider = "bedrock"
region   = "us-east-1"
```

**No secret is ever stored in the config.** `api_key_env` names the environment variable that holds
the API key; the provider reads it from the environment at call time. A config with
`provider = "none"` (the default) leaves the chat off.

Model ids differ between the two. Bedrock ids carry a routing prefix ahead of `anthropic.`, so the
default model is `claude-fable-5-1` on the first-party API and
`global.anthropic.claude-fable-5-1` on Bedrock. The `global.` prefix routes the request
cross-region at no pricing premium; `us.`, `eu.`, `jp.`, and `apac.` pin an inference profile to
one geography and cost 10% more.

### Bedrock credentials

Bedrock requests go to `InvokeModel` on `bedrock-runtime.<region>.amazonaws.com`, carrying the same
Messages body as the first-party API. They authenticate one of two ways. When the environment
variable named by `api_key_env` (default `AWS_BEARER_TOKEN_BEDROCK`) holds a Bedrock API key, ciq
sends it as an `Authorization: Bearer` header. When it is unset, ciq signs the request with AWS SigV4 using static credentials, resolved the
way the AWS SDK default chain resolves them: `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` /
`AWS_SESSION_TOKEN` first, then static keys for the named profile in `~/.aws/credentials`, then
static keys for it in `~/.aws/config`, and only then the profile's `credential_process` command. The
`AWS_SHARED_CREDENTIALS_FILE` and `AWS_CONFIG_FILE` overrides are honored.

A `credential_process` profile works: ciq runs the command, reads the JSON credentials it prints,
and signs with them. Nothing is cached, so each request picks up a refreshed credential. A profile
that instead relies on SSO, role assumption, or a web identity token is reported as unsupported with
a message naming the mechanism, because a blocking client with no AWS SDK cannot complete those
handshakes. Refresh those into the environment or into a profile ciq can read before launching.

The Bedrock endpoint is regional. The region comes from `[ai] region`, then `AWS_REGION`, then
`AWS_DEFAULT_REGION`, then the profile's own `region`, and finally `us-east-1`.

## Safety

The generated SQL is not privileged. A `DROP TABLE`, a multi-statement reply, or any non-`SELECT` is
rejected by the same preprocess guard that vets a typed query and never reaches the engine.
Suggestions are checked by that guard twice over: once before a row is drawn, and again on the
normal dispatch path when you pick one.

See the [Quick Reference](../quick-reference.md) for the complete set.
