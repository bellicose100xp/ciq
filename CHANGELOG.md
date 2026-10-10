# Changelog

All notable changes to ciq are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and ciq uses
[pre-1.0 versioning](CLAUDE.md#versioning--releasing): minor `0.X.0` for features and breaking
changes alike, patch `0.minor.Y` for fixes, refactors, polish, and docs.

## [Unreleased]

### Added
- **Vim editing is the same in every query box.** The five Simple-mode clause panes now behave as
  one editor with five lines: in Normal mode `j`/`k` (and `Up`/`Down`/`Enter`) step between panes,
  `gg`/`G` jump to `SELECT`/`LIMIT`, and the mode and the yank register travel with the focus, so
  moving panes keeps you in Normal mode and a `yy` in one pane pastes with `p` in another. The same
  carry runs across the `Ctrl+Q` toggle. `Alt+J`/`Alt+K` keep the mode too.
- **Yank and paste.** `yy`/`Y`, `y<motion>` (`yw`, `y$`, `yf<c>`, …), and `y` with a text object
  (`yiw`, `yi"`, `ya(`, …) fill a register and copy it to the system clipboard through OSC 52;
  `p`/`P` paste it after/at the cursor. Every `d`/`c` edit fills the register too, so `dw` then
  `p` moves a word. The register holds text: a yanked line pastes inline.
- **`/` in Normal mode opens the row search**, as it does in jiq.
- A bracketed paste while a query box is in Normal mode now inserts the text, as vim does; it
  was silently dropped.
- A new [Vim editing](docs/features/vim-editing.md) page documents the full key set; the quick
  reference and the live-grid page no longer say `Esc` quits.

### Fixed
- A Simple-mode pane could grow a hidden second row: `Enter`, `o`/`O`, a pasted newline, or a
  multi-line clause coming back from Power mode put text on a line the one-row pane never showed.
  Panes are now single-line editors: `Enter` is a no-op, `o`/`O` act as `A`/`I`, and newlines
  arriving from a paste, a register, or the simplifier are flattened to spaces.
- `cc` on a multi-line Power query removed the row like `dd`; it now empties the line and leaves
  the cursor on it in Insert mode.
- The autocomplete popup re-opened after `Esc` and kept capturing `Enter`, `Up`, `Down`, and `Tab`
  in Normal mode. It is now an Insert-mode surface and stays closed in Normal mode.
- A focused `GROUP BY` or `ORDER BY` pane drew its text flush against the label; the label column
  now leaves a gutter after the accent bar.

## [0.4.0] - 2026-09-10

### Added
- **Light-terminal palette** — ciq now carries a second palette for light backgrounds, with the
  same hue families as the dark one deepened so every column color, accent, and hint holds at least
  3:1 contrast against white. Popup surfaces and the focused-pane tint invert with it, so they read
  as light elevations instead of dark islands. The palette is picked once at startup: `[theme]
  mode = "auto"` (the default) asks the terminal for its background color and falls back to dark
  when it does not answer; `"light"` / `"dark"` pin it. The `[theme] mode` key was previously
  parsed and ignored.
- **The AI feature now really calls a model.** Until now the AI surface was wired end to end but its
  provider was a stub that answered "not built into this binary", so nothing but the mock ever
  replied. ciq now ships a blocking HTTP client speaking the Anthropic Messages API, with two
  providers behind `[ai] provider`. `"anthropic"` calls the first-party API and reads its key from
  the environment variable `api_key_env` names. `"bedrock"` calls Claude in Amazon Bedrock through
  `InvokeModel` on the regional `bedrock-runtime` endpoint, authenticating with a bearer token from
  that same variable
  (default `AWS_BEARER_TOKEN_BEDROCK`) or, when it is unset, signing the request with AWS SigV4
  using static credentials resolved the way the AWS SDK default chain resolves them: the AWS
  environment variables, then static keys in the named profile of `~/.aws/credentials` and
  `~/.aws/config`, then that profile's `credential_process` command, whose JSON output is read
  fresh on every request. A profile that needs SSO or role assumption is reported as unsupported
  with a message naming the mechanism, so you know to refresh credentials rather than guessing at
  a network problem. The region comes from `[ai] region`, `AWS_REGION`, `AWS_DEFAULT_REGION`, the
  profile's `region`, or `us-east-1`. No secret is stored in the config: the key is read from the
  environment on every request, so one exported after launch is picked up without a restart.
- **The AI chat is open by default and shares the keyboard.** It appears at the bottom right of the
  results pane above the query box (jiq's placement, about 70% wide and 40% tall) as soon as the
  CSV loads, without stealing the keyboard: SQL typing goes to the bar until you click the chat or
  press `Ctrl+G`. Clicks move focus rather than closing the chat, so you can click where you want
  to type, and the input line shows a block cursor whenever the chat has the keyboard.
  `[ai] open_on_start = false` restores the closed-on-launch behavior. A failing query pops the chat
  up if it is hidden, so the fixes are always on screen.
- **Two chords with one job each.** `Ctrl+A` shows or hides the chat and nothing else. `Ctrl+G`
  moves the keyboard between the query bar and the chat, opening it if hidden; `Esc` from the chat
  goes back to the bar with the chat still visible. Before, `Ctrl+A` focused a visible chat on one
  press and closed it on the next, which made it easy to lose the box while trying to type in it.
- **The AI box matches jiq's.** Prose and SQL wrap to the box width instead of being cut off, each
  suggestion is a numbered block with its note on a dimmed line, exchanges are separated by a blank
  line, the top border names the provider and model, and the bottom border carries the shortcut
  legend for the current focus. Applying a suggestion now keeps you in the five-pane Simple mode
  when the query fits the panes, switching to SQL mode only for a CTE, join, subquery, or `HAVING`.
- **The AI popup is now a conversation** (`Ctrl+A`). The one-shot ask box became a chat: you type a
  question, the model answers with a line or two of plain prose plus zero or more pickable SQL
  suggestions, and the next question goes out with the prior turns attached, so "now only the EU
  region, and sort descending" refines the answer you just got instead of starting over. The
  transcript survives closing the popup, so reopening with `Ctrl+A` resumes the same thread and
  `Ctrl+L` starts a new one. A question can be answered with prose alone (ask what a column probably
  contains) and an empty suggestion list is a normal reply. `Enter` sends the typed line, or applies
  the suggestion under the cursor when the line is blank; `Alt+1` through `Alt+5` apply that row of
  the most recent answer; `Alt+Up` / `Alt+Down` (or `Alt+J` / `Alt+K`) walk every suggestion in the
  whole transcript, so you can scroll back and apply an older one; `Up` / `Down`, `PageUp` /
  `PageDown` and the wheel scroll the conversation. Applying a suggestion drops its SQL into the bar
  through the normal dispatch path and leaves the chat open, so the conversation continues against
  the new result. Suggestions are labelled `fix` or `optimize` and colored by kind, and each is put
  through the same read-only single-`SELECT` guard as a typed query *before* it is drawn, so an
  unsafe or multi-statement suggestion is never even offered. A failing query appends a turn
  reporting the error and asking for fixes, opening the chat if it is hidden, but never while
  another request is in flight; a successful query says nothing.
- **The `[ai]` config section grew** to cover the live providers: `provider` gained `"bedrock"`,
  and `region`, `profile`, `effort`, `max_tokens`, `request_timeout_secs`, `extra_instructions`, and
  `max_context_length` are new. `max_context_length` is the per-request character budget, spent on
  the conversation turns sent with a message as well as the result sample or error text inside it.
  The default model is now Claude Fable 5.1 (`claude-fable-5-1`, or
  `global.anthropic.claude-fable-5-1` on Bedrock, whose ids take a routing prefix) at
  `effort = "low"`: thinking is always on for that
  family, so effort is the lever that decides how long and how expensively it thinks, and writing
  one SQL suggestion against a schema it was already handed does not need deep reasoning. Unknown
  keys in the section are still rejected, which is what turns `api_key = "sk-…"` into a parse error
  instead of a secret sitting in a plaintext config.

## [0.3.0] - 2026-07-14

### Added
- **Per-column colors** — each grid column paints in its own pastel hue, rotating through an
  eight-color palette, so adjacent columns read as distinct bands when you scan a wide result. The
  sticky header label carries the same hue as the data below it, and the hue is keyed on the
  absolute column index, so a column keeps its color as the grid scrolls horizontally. A genuine
  SQL `NULL` still renders dim and a search match still takes the highlight band; both override the
  column hue. The `Ctrl+O` console dump uses the same palette, so it matches what was on screen.
  Only the on-screen page is colored, so this adds no per-row cost against a large result.

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
- **AI natural-language-to-SQL** (`Ctrl+A`) — describe a query in plain English against the live
  schema (requires a configured provider).
- **Output modes** — dump a result to CSV, TSV, JSON, or Markdown.
- **Mouse support** — wheel and trackpad scrolling, click-to-focus, click-to-position the cursor,
  popup row selection, double-click to accept an autocomplete suggestion or toggle a palette
  column, single-click history recall, click-outside-to-dismiss for the facet/history/AI popups,
  and a hover highlight with a bright left accent bar that follows the pointer across grid rows.
- **Release flow** — cargo-dist shell installer (curl-based) for macOS and Linux, built by the
  `Release` workflow on a `vX.Y.Z` tag.

[Unreleased]: https://github.com/bellicose100xp/ciq/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/bellicose100xp/ciq/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/bellicose100xp/ciq/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/bellicose100xp/ciq/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/bellicose100xp/ciq/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/bellicose100xp/ciq/releases/tag/v0.1.0
