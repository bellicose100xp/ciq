# ciq — Human validation smoke script

The headless suite (`cargo test --all-features -- --test-threads=1`) proves all logic and the
*logical* cell grid (`TestBackend`). This file lists the small residue that only a real terminal can
confirm — the canonical §4.7 human surface. Per the plan these checks **batch into the P4/P5 gate**;
they are not separate blocking stops as each phase lands.

Run with a released/`cargo run --release -- <file.csv>` build against a CSV that has a
low-cardinality text column (e.g. `status`) and a date column, in **both** a light and a dark
terminal.

**Screen layout (top -> bottom):** the bordered results pane fills the screen (its border title
shows the `delim , | header on` dialect summary; its first inner row is the single sticky header
of `name (badge)` column labels), then the **query bar near the bottom** (`> ` prompt + a
**multiline** editing area with a **visible block cursor**), then the status line at the very
bottom. The query *input* is at the bottom; all popups (autocomplete / palette / facet / history /
AI) anchor **just above** the query bar and grow upward over the results pane. The query bar grows
downward by one row per added line (capped at 5 rows, then it scrolls internally).

## Phase 3 — autocomplete popup (P3.6 / P3.7)

The headless snapshot proves the popup's logical cells only (which glyphs / candidates / the
right-aligned type-hint land where). It does NOT prove real glyphs, on-screen placement, or color
polarity. Confirm by hand:

1. **Popup opens + column candidates.** Type `SELECT st`. A popup appears just above the bottom
   query bar listing columns matching `st` (e.g. `status`), each with its type badge right-aligned
   (`txt`, `int`, `date`, …). Confirm the badge column is legible (not clipped, readable color).
2. **Tab inserts the selection.** With `status` highlighted, press `Tab`. The bar becomes
   `SELECT status` and the popup closes. No flicker, no stray characters.
3. **Arrow selection.** Type `SELECT ` (trailing space) to list all columns. Press Down/Up and
   confirm the highlighted row moves (and wraps at the ends), reverse-video reads clearly, and the
   Down arrow moves the *selection* (it does NOT jump focus to the results grid while the popup is
   open).
4. **Esc dismisses, does not quit.** With the popup open, press `Esc` — the popup closes and the
   app stays running. Press `Esc` again (popup now closed) — the app quits.
5. **Keyword-collision quoting.** If your CSV has a column whose name is a SQL keyword (or add one),
   type `SELECT or…` and accept it — confirm it inserts as `"order"` (quoted), not `order`.
6. **Value completion (P3.7).** Type `SELECT * FROM t WHERE status = '`. After a beat the popup
   shows the *distinct actual values* of `status` (fetched through the worker). Type a letter to
   filter; accept one and confirm it inserts as a single-quoted literal, e.g. `'active'`.
7. **Placement / overflow.** Resize the window narrow and tall, and short and wide, while a popup is
   open. Confirm the popup stays anchored just above the bottom query bar, grows upward without
   overflowing the top of the pane, and does not corrupt the grid behind it.
8. **Color polarity.** Repeat 1 and 6 in a light terminal and a dark terminal. Confirm the popup
   border, the selected-row highlight, and the dimmed type-hint column are all legible in both
   (the §4.7 polarity check).

## Phase 4 — single type-annotated header + bottom query bar (P4.1 / layout)

The grid's one sticky header carries each column's `name (badge)` label (the type badge folded in);
there is **no separate schema-bar row** — the old design showed the column names twice (a dimmed
name row above a bold name row), which is gone. The dialect summary moved to the pane border title.
The headless snapshots prove the single header's logical cells and that the query bar is the
second-to-last row; they do NOT prove real glyphs or color polarity. Confirm by hand:

1. **One header, type-annotated.** Run a query that returns a grid (e.g. `SELECT * FROM t`). The
   first inner row of the results pane is a single row of `name (badge)` labels (e.g.
   `id (int)   name (txt)   amount (num)`), each sitting dead-on over its data column. Confirm the
   column names appear **exactly once** — there is no duplicate header row above or below it.
2. **Header stays sticky.** Scroll the grid vertically (Down/Up while the grid has focus) and
   confirm the header row stays put while the body scrolls under it.
3. **Query bar at the bottom.** Confirm the `> ` query input sits near the **bottom** of the screen
   (with the status line below it), not at the top. Typing updates the bar at the bottom and the
   grid above it refreshes live.
4. **Delimiter/header summary.** The pane border title reads `delim , | header on` (or the actual
   delimiter for your file; a TSV shows `delim \t`). Confirm it is legible in a light and a dark
   terminal (the §4.7 polarity check).

## Phase 4 — column palette (P4.2-P4.5)

The headless suite proves the palette's generated SQL (`emit` goldens for both quoting surfaces),
the toggle/reorder/filter/ownership state machine, the ownership byte-compare, and the popup's
*logical* cells (80x24 `TestBackend` snapshot — which checkboxes / column names / right-aligned type
badges land where). It does NOT prove the drawn popup glyphs, the real `Space`/arrow chords as the
terminal delivers them, or the Replace-transition feel. Confirm by hand (open a CSV with a few
typed columns including a reserved-word column if you can, e.g. `order`):

1. **Open + checkboxes.** Press `Ctrl+K`. A bordered "columns" popup appears just above the bottom query bar
   listing every column with a `[ ]` checkbox and a right-aligned type badge (`int`/`txt`/`date`/…).
   Confirm the badges are legible and the box does not overflow the screen edge.
2. **Space toggles.** Move the cursor (Up/Down) to a column and press `Space`. Confirm the checkbox
   flips to `[x]` (accented/bold so the selection set reads at a glance) and `Space` again clears it.
3. **Typing filters.** Type a few letters. Confirm the list narrows to columns matching (fuzzy), the
   popup title shows the needle (`columns: <needle>`), and the **query bar stays untouched** (typing
   filters the palette, it does not edit the bar). `Backspace` widens the list again.
4. **Left/Right reorder.** Check two columns, move the cursor onto a checked one, and press
   `Left`/`Right`. Confirm its position in the eventual projection moves earlier/later (verify via
   step 5's emitted `SELECT`).
5. **Enter emits.** Press `Enter`. Confirm the popup closes, the bar fills with the generated
   `SELECT <picked cols> FROM t LIMIT n` (picked columns in your selection order; a reserved-word
   column appears quoted as `"order"`), and the grid updates to that query within a debounce tick.
6. **Esc closes, does not quit.** Reopen with `Ctrl+K`, press `Esc` — the popup closes and the app
   stays running. (Esc only quits when no popup/palette is open.)
7. **Replace transition (the UX cliff to eyeball).** Hand-type a query with a WHERE, e.g.
   `SELECT id FROM t WHERE region='EU'`, then open the palette and emit/replace. Confirm accepting
   Replace **discards the WHERE** and snaps the bar to the palette's generated query (correct-by-
   construction per §0/D3, but verify it reads as a deliberate replace, not a silent data loss).
8. **Color polarity.** Repeat 1-2 in a light and a dark terminal. Confirm the popup border, the
   cursor-row reverse-video highlight, the checked-checkbox accent, and the dimmed type-badge column
   are all legible in both (the §4.7 polarity check).

## Phase 4 — instant facets (P4.6)

The headless suite proves the type-aware facet SQL (`build_facet_sql` goldens per column type), the
result parse (`FacetState`), the histogram bar-width math, the `format_facets` lines, the popup's
*logical* cells (80x24 `TestBackend` snapshot), and the full worker round-trip (the App dispatches
the facet on the same channel, routes the response to the popup not the grid, on a real engine over
a fixture). It does NOT prove the drawn popup glyphs, the bar color, on-screen placement, or the
real `f`/`Esc` chords as the terminal delivers them. Confirm by hand (open a CSV with a
low-cardinality text column like `region` and a numeric/date column like `amount`/`created_at`):

1. **Open a facet.** Run a query so the grid has rows, press `Down` to focus the results pane, then
   press `f`. A bordered "facet: <column> (<badge>)" popup appears just above the bottom query bar for the
   leftmost visible column. Confirm it briefly shows "computing…" then fills (the worker round-trip).
2. **Numeric/date summary.** With the focused column numeric or a date, confirm the popup shows
   `min` / `max` / `distinct` / `nulls` lines, the values legible and the labels dimmed.
3. **Text histogram.** Scroll the grid (`Right`) so a low-cardinality text column (e.g. `region`) is
   leftmost, press `f`. Confirm the popup shows `distinct` / `nulls` then a `value  count |####` bar
   per top value, the bars proportional (the most-frequent value has the longest bar) and the order
   stable (highest count first).
4. **Esc closes, does not quit.** Press `Esc` — the popup closes and the app stays running. (Esc
   only quits when no popup is open.) Any other key (e.g. an arrow) also dismisses it and resumes
   grid navigation.
5. **Color polarity.** Repeat 2-3 in a light and a dark terminal. Confirm the popup border, the
   accented stat values, the histogram bar color, and the dimmed labels are all legible in both (the
   §4.7 polarity check).

## Phase 4 — output modes / OSC 52 clipboard (P4.8)

The headless suite proves the emitted bytes of every format (`render_output` goldens) AND the full
`--output csv|tsv|json|markdown` CLI path end-to-end against `tests/fixtures/sample.csv`. The only
residue a terminal must confirm is the OSC 52 *clipboard write* — `clipboard::osc52::copy` emits the
escape sequence to the real terminal, which no in-memory backend can receive (the §4.7 row 4
clipboard / OSC 52 check). The escape-string builder (`encode_osc52`) is round-trip tested headless;
this only confirms the terminal actually honors it. Confirm by hand:

1. **Copy the result.** With a result on screen, trigger the in-app copy (the export/copy key, once
   wired). Switch to another app and paste — confirm the clipboard now holds the rendered result in
   the selected format (e.g. a CSV block whose fields/quoting match what `--output csv` prints).
2. **Over SSH / multiplexer.** Repeat 1 inside `tmux`/`ssh` if you use one — OSC 52 is the path that
   carries the clipboard across the wire, so confirm the paste still lands (the terminal/multiplexer
   may need OSC 52 forwarding enabled).

## Phase 5 — query history popup (P5.2)

The headless suite proves the history ring (add/dedupe/recall/navigate/filter), the on-disk
round-trip (against a tempdir), the pure key->action map, and the popup blit (TestBackend snapshot).
What a real terminal must confirm is the §4.7 residue — true-terminal glyphs/placement, the cursor
reverse-video color, the real `Ctrl+R` chord, and color polarity. Confirm by hand:

1. **Open the popup.** With a few queries run, press `Ctrl+R`. A bordered "history (N)" popup appears
   just above the bottom query bar, listing prior queries newest-first, the top row highlighted
   (reverse-video).
2. **Navigate + recall.** Press `Down`/`Up` to move the highlight; confirm it tracks and the window
   scrolls once you pass the bottom. Press `Enter` on an entry — the popup closes and that SQL drops
   into the query bar, and the grid updates (it fired through the normal debounce/dispatch path).
3. **Filter.** Reopen (`Ctrl+R`) and type a few chars — confirm the title shows the needle + a
   `(matched/total)` count and the list narrows to fuzzy-matching entries; a non-matching needle
   shows the dimmed "(no matches)" line. `Backspace` widens it again.
4. **Esc closes, Ctrl-C quits.** `Esc` closes the popup without recalling (the bar is unchanged) and
   the app stays running; `Ctrl+C` from the popup quits.
5. **Persistence across sessions.** Run a query, quit, relaunch on the same file — confirm `Ctrl+R`
   shows the query from the prior session (it was written to the on-disk history file). If you set
   `[history] enabled = false` in `~/.config/ciq/config.toml`, confirm history is session-only (the
   file is not written).
6. **Color polarity.** Repeat 1-3 in a light and a dark terminal — confirm the popup border, the
   highlighted row, and the dimmed title/no-matches line are all legible in both (the §4.7 check).

## Phase 5 — AI NL→SQL popup (P5.1)

The headless suite proves the pure prompt builder (schema grounding), the popup state machine, the
popup blit (TestBackend snapshot), the AI thread round-trip (with the mock provider — no network),
and that a generated query flows through the existing read-only guard + dispatch path (a `DROP`/
multi-statement reply is rejected). What a real terminal must confirm is the §4.7 residue — the
real `Ctrl+G` chord, true-terminal glyphs/placement, the magenta popup border, and color polarity.
**This requires a configured provider** (see below); without one the chord is a no-op, which is
itself worth confirming. **NL→SQL answer quality is a RECOMMENDED spot-check, NOT a blocking gate**
(model output is non-deterministic and out of ciq's control). Confirm by hand:

1. **Chord is a no-op when unconfigured.** With no `[ai]` block (or `enabled = false`), press
   `Ctrl+G` — confirm nothing happens (the feature is off, no popup).
2. **Popup opens (configured).** With `[ai] enabled = true`, `provider = "anthropic"`, and the API
   key exported in the env var named by `[ai] api_key_env` (default `ANTHROPIC_API_KEY` — never put
   the key in the config file), press `Ctrl+G`. A bordered "ask AI" popup appears under the query
   bar with a `> ` prompt. Type a request (e.g. `rows where status is active`) and confirm the text
   lands in the popup, NOT in the query bar.
3. **Esc closes, does not quit; Ctrl-C quits.** With the popup open, `Esc` closes it and the app
   stays running; `Ctrl+C` from the popup quits.
4. **Generate (RECOMMENDED quality spot-check).** Press `Enter`. Confirm the popup shows
   `generating…`, then either drops a `SELECT …` into the query bar (popup closes, the grid updates
   through the normal debounce path) or shows an `error: …` line (popup stays open to retry).
   *Recommended, not blocking:* eyeball whether the generated SQL is a reasonable answer to the
   request — but a poor answer is a model-quality issue, not a ciq bug.
5. **Read-only guard still applies.** If the model ever returns non-SELECT SQL, confirm the status
   line shows `read-only SELECT queries only` and the table is unchanged (the AI cannot smuggle DML
   past the existing guard — this is enforced headlessly, but worth seeing once live).
6. **Color polarity.** Repeat 2 in a light and a dark terminal — confirm the magenta popup border,
   the prompt text, the `generating…`/success/error lines are all legible in both (the §4.7 check).

**Note (current build):** the real provider's HTTP client is not compiled into this binary (no
network dependency yet — see `src/ai/provider.rs`), so step 4 will surface a clear "the live HTTP
provider … is not built into this binary" message rather than a real completion. The chord/popup/
guard/polarity checks (1-3, 5, 6) are fully exercisable now; the live-completion quality check (4)
applies once the HTTP body is wired.

## Post-5 input UX — multiline query box + visible cursor (tui-textarea)

The query bar is now a `tui_textarea`-backed editor (multiline, with a visible block cursor cell).
The headless suite proves the joined text, the byte<->(row,col) bridge, the multiline routing, and
that a reverse-video cursor cell lands in the `TestBackend` buffer. It does NOT prove the real
on-screen cursor glyph, the felt typing experience, or color polarity. Confirm by hand:

1. **Visible cursor.** Open a CSV. Without typing, confirm a block cursor is visible at the start of
   the query bar (the old build showed no cursor at all). Type a few characters and confirm the
   cursor sits after the last typed character and moves as you type.
2. **Cursor follows edits.** Use Left/Right/Home/End and Backspace/Delete; confirm the cursor lands
   where expected and the block cursor is always drawn over the correct cell (no off-by-one, no
   missing cursor at end-of-line).
3. **Enter adds a newline (no submit).** Type `SELECT *`, press `Enter`, type `FROM t`. Confirm the
   query bar grows to two rows (`> SELECT *` on top, `  FROM t` below), the query still runs live on
   debounce (the grid updates — there is no "submit" key), and Enter never clears or runs-and-resets
   the bar. Shift+Enter behaves the same (newline).
4. **Multiline navigation.** In a two-line query, Up/Down move the cursor between lines. From the
   **last** line, Down hands focus to the results grid (as before); from the first line, Up stays in
   the bar.
5. **Bar growth + scroll cap.** Add several lines (Enter repeatedly). Confirm the bar grows one row
   per line up to ~5 rows, the results pane shrinks to make room, and beyond the cap the textarea
   scrolls internally rather than growing further. Confirm the status line stays the very last row.
6. **Color polarity.** Repeat 1-3 in a light terminal and a dark terminal — confirm the reverse-video
   cursor cell and the query text are legible in both (the §4.7 polarity check).
