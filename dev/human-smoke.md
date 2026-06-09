# ciq — Human validation smoke script

The headless suite (`cargo test --all-features -- --test-threads=1`) proves all logic and the
*logical* cell grid (`TestBackend`). This file lists the small residue that only a real terminal can
confirm — the canonical §4.7 human surface. Per the plan these checks **batch into the P4/P5 gate**;
they are not separate blocking stops as each phase lands.

Run with a released/`cargo run --release -- <file.csv>` build against a CSV that has a
low-cardinality text column (e.g. `status`) and a date column, in **both** a light and a dark
terminal.

**Screen layout (top -> bottom):** the bordered results pane fills the screen (its border title
shows the `delim , | header on` dialect summary on the left and the jiq-style `<rendered>/<total>`
**row counter** on the right — `12/12` when the result is uncapped, `1000+` when ciq applied its
viewport `LIMIT`; its first inner row is the single sticky header of `name (badge)` column labels,
no separate truncation-banner row anymore). Then the **bordered query box near the bottom** (`> `
prompt + a **multiline** editing area with a **visible block cursor**) whose **top border carries
the vim mode badge** (`INSERT` / `NORMAL` / per-mode color, jiq-style) and whose **bottom border
carries the context-sensitive keyboard help hints, centered**, then the status line at the very
bottom. The query *input* is near the bottom; all popups (autocomplete / palette / facet / history
/ AI) anchor **just above** the query box and grow upward over the results pane. The query box
grows downward by one row per added line (capped at 5 text rows, then it scrolls internally).

**Bright "galaxy" theme.** Borders are **focus-aware**: the focused pane (results pane vs query box)
lights up in **bright cyan** (`Color::Rgb(0, 217, 255)`); the unfocused pane recedes in muted slate.
Popups (autocomplete / column palette / facets / history) use the same bright cyan border; the AI
popup uses purple. Every color is the verbatim galaxy palette from jiq's `theme/galaxy.rs::galaxy_dark`.

## Post-5 UX — keyboard-shortcut help + vim mode badge on the query box border (§4.1)

The headless snapshot proves the *logical* cells (which key/desc text lands on the box's bottom
border per context, that trailing hints drop on a narrow box, that the mode badge rides the box's
TOP border, and that the bottom-border hints are centered). It does NOT prove real glyphs, color
polarity, or the on-screen feel. Confirm by hand (light + dark terminal):

1. **Hints are present on the box bottom border, centered, + read cleanly.** The query box's
   **bottom border** carries a legend of `key  desc` pairs joined by a `\u{2022}` bullet (like jiq),
   not a separate row. The legend is **centered** on the bottom border (no longer left-aligned).
   Keys stand out (bright cyan + bold), descriptions are normal text, the bullet is muted. No emoji.
2. **Mode badge rides the TOP border.** With the query bar focused, the box's **top border** shows
   the vim mode badge (`INSERT` left-aligned, in a per-mode color). Press `Esc` -> the badge on the
   top border flips to `NORMAL` (yellow) and the bottom-border hints swap to vim motions (`hjkl
   move`, `i insert`, `dd/dw delete`). Press `i` -> back to `INSERT` (cyan) on the top border. The
   badge is NOT duplicated on the bottom border anymore.
3. **Per-mode badge color.** Confirm Insert is cyan, Normal is yellow, an operator-pending state
   like `d(` is green, and a char-search-pending state like `f` is pink (jiq's per-mode palette).
4. **Hints follow the open popup.** Open each popup and confirm the box bottom border shows that
   popup's keys: autocomplete (`Tab accept`, `Up/Down select`, `Esc close`); `Ctrl+P` palette
   (`Space toggle`, `Left/Right reorder`, `Enter apply`); `Ctrl+R` history (`Enter recall`);
   `Ctrl+A` AI (`Enter generate`); `f` facet in the results pane (`Esc close`). NOTE on tmux:
   `Ctrl+A` is sometimes the screen-style tmux prefix; if your tmux config uses it, rebind tmux's
   prefix or use the mouse to open the AI popup.
4a. **Simple-mode bar (no popup) shows pane-nav and Tab=\t.** With no popup open and the query bar
   in Insert mode, the bottom border reads `Alt+↑↓ panes` followed by `Tab \t`, `Ctrl+A AI`,
   `Ctrl+P columns`, `Ctrl+R history`, `Ctrl+T results`, `Ctrl+Q SQL`, `Esc vim`, `Ctrl+C quit`.
   Power mode shows `Tab complete` instead (Tab in the textarea opens / accepts an autocomplete
   suggestion).
4b. **Cursor only on the focused pane + Alt-nav is bounded.** In Simple mode, exactly ONE pane
   shows a reverse-video block cursor — the focused one. `Alt+J` / `Alt+Down` move focus forward
   (SELECT → WHERE → GROUP BY → ORDER BY → LIMIT), BOUNDED — at LIMIT it stays put (no wrap).
   `Alt+K` / `Alt+Up` move back, bounded at SELECT. Plain Tab no longer cycles panes (it inserts
   a literal `\t`); plain Up/Down in the bar are no-ops.
4c. **Popup-aware Up/Down/Tab.** With the autocomplete popup open: Up/Down move the highlighted
   suggestion (NOT pane focus); Tab and Enter accept the highlight (insert + close); Esc closes
   the popup but does NOT flip vim Insert -> Normal. With the popup closed: `Alt+↑↓` (or
   `Alt+J/K`) move pane focus, bounded; Tab inserts a literal `\t`; plain Up/Down are no-ops;
   Esc flips Insert -> Normal.
4d. **Ctrl+T toggles focus.** From the query bar, `Ctrl+T` shifts focus to the results grid (the
   border styling swaps and the bottom hints change to scroll/page/column). Press `Ctrl+T` again
   to come back to the query bar with the same Simple pane focused as before. Works in both
   Simple and Power modes.
5. **Results-pane hints.** Press `Ctrl+T` from the query bar (or `Down` past the last line in
   Power mode) to focus the grid -> the bottom border shows scroll/page/column hints
   (`Up/Down scroll`, `PgUp/PgDn page`, `Left/Right columns`, `f facet`, `Ctrl+T query`,
   `Ctrl+C quit`) and **no** mode badge on the top border (the editor is not the focused surface).
   Press `Ctrl+T` again to return.
6. **Narrow terminal.** Shrink the terminal width; confirm low-priority trailing hints drop whole
   (no clipped mid-word text, no overflow past the box's right corner) while the highest-priority
   hint stays. The mode badge stays as long as the label fits the top border.

## Post-5 UX — focus-aware borders (bright cyan)

The headless suite proves the focused pane's border carries `Color::Rgb(0, 217, 255)` and the
unfocused pane's border carries the muted-slate fg. It does NOT prove the felt brightness or color
polarity. Confirm by hand (light + dark terminal):

1. **Focused pane is bright cyan.** With focus on the query bar, the query box's border is bright
   cyan; the results pane border is muted slate. Press Down past the last line to hand focus to the
   results pane -> the cyan accent jumps to the results pane border, and the query box dims.
2. **Popup borders are bright cyan.** Open the autocomplete / palette / history popups — confirm
   their borders are bright cyan (the AI popup is purple instead).
3. **Color polarity.** Repeat 1 in a light and a dark terminal — the bright-cyan accent must read
   clearly in both.

## Post-5 UX — row counter on the results pane border (jiq-style)

The headless suite proves the row counter text (`<rendered>/<total>` for uncapped, `<rendered>+`
for ciq-capped) lands on the top-right of the results pane border, and that there is no separate
"showing first N rows" interior banner row anymore. It does NOT prove the on-screen reading. Confirm
by hand:

1. **Uncapped result.** Run a query that returns < 1000 rows (e.g. `SELECT * FROM t LIMIT 12`).
   Confirm the top-right of the results pane border reads `12/12` and there is no truncation banner
   row inside the pane (the body gets the row back).
2. **Capped result.** Run `SELECT * FROM t` against the showcase fixture (5000 rows). Confirm the
   counter reads `1000+` (the `+` carries the cap signal — the old `showing first 1000 rows` banner
   row is gone). The grid header sits at the very top of the inner pane now, not below a banner.
3. **Stale dim.** Trigger an error (e.g. `SELECT bogus`). Confirm the kept grid dims AND the row
   counter on the border dims along with it (it is part of the kept-result polarity).

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

## Phase 4 — SELECT-pane column picker (user-locked redesign 2026-06-09)

The picker is anchored to the SELECT pane: `Ctrl+P` only opens the popup when focus is on
SELECT. Toggling a column inside the popup **rewrites the SELECT-pane text immediately** — the
popup is the live editor for the projection, not an accept/cancel dialog. The popup uses a
distinct **magenta** accent so it reads as different from the cyan-default popups (autocomplete,
history, AI, facet).

The headless suite proves the state machine + the popup's logical cells (80x24 `TestBackend`
snapshot). Confirm by hand:

1. **Scope.** With focus on the WHERE pane (the launch default), press `Ctrl+P` — nothing
   happens (silent no-op; the picker is anchored to SELECT). Move focus up to the SELECT pane
   (`Alt+Up` or `Alt+K`), press `Ctrl+P` — the popup opens.
2. **Pre-checked from SELECT.** With SELECT showing `*` (the default), every checkbox is
   checked on open. Type `id, name` into SELECT first (close the popup, move to SELECT, type)
   then reopen — only those two are checked.
3. **Live toggle.** Move the cursor (`↑`/`↓`) and press `Space` (or `Tab`) to toggle. Confirm
   the SELECT-pane text behind the popup rewrites IMMEDIATELY (no Enter / no apply step) and
   the grid filters within a debounce tick. A reserved-word column like `order` appears in
   SELECT quoted as `"order"`.
4. **Bulk ops.** `Ctrl+A` checks all → SELECT becomes `*`. `Ctrl+X` deselects all → SELECT
   empties (the composer falls back to `*`, so the grid keeps showing the full table).
   `Ctrl+I` inverts the current set.
5. **Close.** `Enter` or `Esc` closes the popup. The SELECT-pane text the toggles shaped stays
   put; closing is just "done."
6. **Distinct theme.** The popup's border + title are **magenta** (vs the cyan border every
   other popup uses). Confirm it reads as visually separate.
7. **Bottom-border hints.** The popup's own bottom border shows
   `Space/Tab toggle • ↑↓ nav • Enter/Esc close • Ctrl+A all • Ctrl+X none • Ctrl+I invert`
   centered (with trailing hints dropped on a narrow popup).
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

## Post-5 input UX — vim keybindings in the query box (ported from jiq)

The query bar is modal like jiq's: it starts in **INSERT** (type normally), and `Esc` drops to
**NORMAL** for vim navigation/edits. The headless suite proves every mode transition + each
motion/edit through synthetic keys and asserts on `text()`/cursor/mode, and that the mode badge
renders on the status line. It does NOT prove the felt modal experience or the per-mode cursor
color on a real terminal. Confirm by hand:

1. **Esc -> Normal, mode indicator.** Open a CSV, type `SELECT * FROM t`. Press `Esc`. Confirm the
   status line (bottom-right) flips from `INSERT` to `NORMAL`, and the block cursor changes color
   (yellow block in Normal vs the plain reverse block in Insert). `Esc` must NOT quit the app
   (only `Ctrl-C` quits now).
2. **Motions (hjkl / w b / 0 $).** In Normal mode, `h`/`l` move left/right, `w`/`b` jump by word,
   `0` to line start, `$` to line end. Confirm no characters are inserted (these are commands, not
   text).
3. **Insert entries (i / a / o).** From Normal: `i` inserts at the cursor, `a` after it, `o` opens
   a new line below — each returns the badge to `INSERT` and lets you type. `Esc` back to Normal.
4. **Edits (x / dd / dw).** In Normal: `x` deletes the char under the cursor, `dd` clears the line,
   `dw` deletes a word. Confirm the grid re-runs live on debounce after each edit (no submit key).
5. **Char-search + text objects (optional spot-check).** `f,` jumps to the next comma; `ci'`
   inside a `'value'` literal clears it and enters Insert; `di(` inside `count(...)` clears the
   args. Confirm the cursor/selection behaves like vim.
6. **Casual typing still just works.** A fresh bar is in INSERT, so opening a file and typing a
   query needs no vim knowledge — the modal layer is opt-in via `Esc`.

## Post-5 input UX — mouse support (scroll, click-to-focus, click-to-position-cursor)

Mouse capture is enabled at terminal init (`EnableMouseCapture` in `event_loop.rs`, the one
shell-exempt edge) and disabled on teardown. The headless suite proves the coordinate mapping
(`LayoutRegions::target_at`) and the routing (`App::on_mouse`) against the recorded `TestBackend`
geometry — scroll offsets, focus, the text cursor column, popup selection — all with synthetic
`MouseEvent`s. It does NOT prove the real terminal delivers wheel/click/drag events, the felt
pointer responsiveness, or that the cursor lands under the actual glyph. Confirm by hand (open a
CSV large enough that the result scrolls, with several columns):

1. **Wheel scroll over the results pane.** Run a query so the grid has many rows. Roll the mouse
   wheel up/down with the pointer over the grid. Confirm the grid body scrolls (a few rows per
   notch), clamped at the top and the last row — the same motion as keyboard PgUp/PgDn.
2. **Horizontal swipe.** With a wide result (more columns than fit), two-finger swipe left/right on
   a trackpad. Confirm the visible columns scroll one column per swipe (column-granular, like
   keyboard Left/Right), clamped at the first/last column.
3. **Click to focus the results pane.** With focus in the query bar, click anywhere in the grid
   body. Confirm focus moves to the results pane (subsequent keyboard scroll/`f`-facet apply to the
   grid). Clicking the pane with no result yet is a no-op (stays on the bar).
4. **Click to focus + position the cursor in the query bar.** Type `SELECT id, name FROM t`, then
   click partway along the text. Confirm focus returns to the bar in INSERT mode and the block
   cursor lands at the clicked character (clicking past the end clamps to the line end; clicking on
   the `> ` prompt clamps to the start). Dragging with the left button positions the cursor the same
   way.
5. **Popup scroll + click (autocomplete).** Type `SELECT ` so the column popup opens. Roll the wheel
   with the pointer over the popup — confirm the selection moves up/down. Click a popup row — confirm
   that row becomes selected and a subsequent `Tab` accepts it. (The column palette `Ctrl+K` popup
   behaves the same for wheel + row click; history/facet/AI popups stay keyboard-driven for now.)
6. **No regression elsewhere.** Confirm clicking/scrolling does not interfere with typing, and that
   the mouse never quits the app or fires a query on its own (queries still run only on debounce).

## Post-5 UX — error keeps last result dimmed (jiq-port)

ciq mirrors jiq's behavior on a query error: the last successful grid stays on screen, **dimmed**,
while the error rides the status line. The headless suite proves the state (`result_is_stale`) and
that the rendered cells carry `Modifier::DIM` (counted in the `TestBackend` buffer). It does NOT
prove the felt brightness shift on a real terminal or that the dim reads correctly in both polarities.
Confirm by hand (light + dark terminal):

1. **Successful grid lands at full brightness.** Run `SELECT * FROM t`. Confirm the header + body
   cells render at normal brightness (the baseline).
2. **Engine error dims the kept grid.** Edit the bar to an unknown column (e.g. `SELECT bogus FROM t`).
   The status line shows `unknown column: "bogus"` (or the did-you-mean form), the **same grid stays
   on screen**, and every header + body cell is visibly dimmer than before. The truncation banner (if
   it was there) stays too, also dimmed.
3. **Preprocess reject dims the kept grid.** From the same successful state, append `;DROP TABLE t`
   to make the bar multi-statement. The status line shows `single statement only` (or `read-only
   SELECT queries only` for plain DML), and the kept grid dims the same way. No empty-state hint
   appears under the error.
4. **A successful query restores full brightness.** Type a valid query (e.g. delete the bad chars).
   The grid is replaced with the new rows at normal brightness — the dim drops the moment the new
   result lands.
5. **First-error has nothing to dim.** Open a CSV and immediately type `;DROP TABLE t` (no prior
   successful grid). Confirm the empty-state hint (`type a SQL query above ...`) stays, the status
   line shows the error, and there is no spurious dim styling.
6. **Color polarity.** Repeat 2 in a light and a dark terminal — confirm the dim grid is still
   readable in both, and the contrast against the un-dimmed status-line error is clear (the §4.7
   polarity check).

---

## Showcase fixture — edge-case tour (tests/fixtures/showcase.csv)

A meatier fixture for driving every TUI edge case by hand. **5000 rows, 14 columns**, generated
deterministically by `dev/gen_showcase.py` (re-run it to regenerate; bump `ROWS` to extend). Columns
and the edge each exercises:

| Column | Type | Edge case it drives |
|---|---|---|
| `id` | BIGINT | right-aligned integer; stable sort key (`ORDER BY id`) |
| `name` | VARCHAR | left-aligned short text |
| `region` | VARCHAR | low-cardinality (EU/NA/APAC/LATAM/MEA) → facets + value completion |
| `status` | VARCHAR | low-cardinality **with ~294 empty cells → SQL NULL** (Q12) |
| `amount` | DOUBLE | right-aligned float; negatives, a few very large (≈9,005,000.99), ~217 NULL |
| `quantity` | BIGINT | right-aligned int, some NULL |
| `active` | BOOLEAN | bool rendering, ~172 NULL |
| `created_at` | DATE | sniffed DATE, right-aligned |
| `updated_at` | TIMESTAMP | sniffed TIMESTAMP (the `value_render` temporal path) |
| `score` | DOUBLE | higher-precision float, edge values 0.0 / 0.0001 |
| `notes` | VARCHAR | **wide** → ellipsis + h-scroll; embedded comma / doubled-quote / newline / CJK / emoji cells |
| `Total ($)` | DOUBLE | column name with space+special chars → `"Total ($)"` quoting (Q3) |
| `order` | BIGINT | reserved-word column name → `"order"` quoting |
| `CreatedBy` | VARCHAR | CamelCase name → case-preservation quoting; another facet target |

Launch: `./target/release/ciq tests/fixtures/showcase.csv`

1. **Type sniffing + per-type alignment.** `SELECT * FROM t` → confirm `id`, `quantity`, `amount`,
   `score`, `Total ($)`, `order`, `created_at`, `updated_at` are **right-aligned**; `name`, `region`,
   `status`, `notes`, `CreatedBy` **left-aligned**. The sticky header shows each column's `name (type)`
   badge.
2. **Truncation banner + vertical scroll.** `SELECT * FROM t` returns 5000 rows but ciq caps the
   view at 1000 → the banner **"showing first 1000 rows (use --output to export all)"** appears.
   Arrow-down / PgDn into the grid and scroll through the body; the type-annotated header stays sticky.
3. **NULL rendering + Q12 semantics.** Scroll until you hit a row with an empty `status`, `amount`,
   or `active` → it renders as a dim `NULL` (distinct from an empty string; right-aligned for the
   numeric column). Then compare: `SELECT count(*) FROM t WHERE status IS NULL` (**294**) vs
   `SELECT count(*) FROM t WHERE status = ''` (**0**) — empty CSV fields ingested as NULL, not `''`.
4. **Wide-column ellipsis + horizontal column scroll.** The `notes` column is long → it truncates
   with `…`. Use `Left`/`Right` (column-granular h-scroll) to bring `notes`, `Total ($)`, `order`,
   `CreatedBy` into view; whole columns shift, never half a cell.
5. **RFC-4180 + unicode cells (truncation must never panic mid-glyph).** Each of these returns ~20
   rows (the cell repeats every 250 rows) — eyeball that they render intact, no panic, no torn glyphs:
   - `SELECT id, notes FROM t WHERE notes LIKE '%Smith, Jr.%'` — embedded commas
   - `SELECT id, notes FROM t WHERE notes LIKE '%ship it%'` — doubled-quote cell (`he said "ship it" today`)
   - `SELECT id, notes FROM t WHERE id = 100` — embedded newline (a single logical row whose cell has two lines)
   - `SELECT id, notes FROM t WHERE notes LIKE '%北京%'` — CJK wide glyphs
   - `SELECT id, notes FROM t WHERE notes LIKE '%🎉%'` (and `'%😀%'`, `'%¡Hola!%'`) — emoji + latin-1
6. **Facets (`f` on a focused grid column).** Arrow into the grid, move to `region` / `status` /
   `CreatedBy`, press `f` → distinct count, null count, and a top-K histogram (each region/creator is
   1000 rows, so the bars are even; `status` shows its NULL count).
7. **Value completion.** Type `SELECT * FROM t WHERE region = '` → popup lists EU/NA/APAC/LATAM/MEA;
   pick one → inserts a quoted literal (`'EU'`). Same for `WHERE status = '` and `WHERE CreatedBy = '`.
8. **Identifier quoting (Q3).** Open the palette (`Ctrl+K`) or autocomplete and select the awkward
   columns → `Total ($)` inserts as `"Total ($)"`, `order` as `"order"`, `CreatedBy` keeps its case.
9. **Palette / history / AI against real data.** `Ctrl+K` to project a subset of the 14 columns;
   `Ctrl+R` to recall a prior query; `Ctrl+G` for NL→SQL (a configured provider answers; otherwise the
   popup shows a clear "not configured" message — the chord/popup itself still works).
10. **Output modes (headless, from a shell).** Confirm byte-level correctness:
    - `./target/release/ciq -q "SELECT id, status, amount FROM t WHERE id IN (17,23,29)" --output json tests/fixtures/showcase.csv` → NULLs are JSON `null`, not `""`.
    - `./target/release/ciq -q "SELECT notes FROM t WHERE id = 50" --output csv tests/fixtures/showcase.csv` → doubled-quote round-trips (`"he said ""ship it"" today"`).
    - Try `--output tsv` and `--output markdown` too.

### Messy headers (Q3 dedup) — tests/fixtures/messy_headers.csv

A tiny 5-row file whose header is `id,name,,name,notes` (a duplicate and an empty name). Load it and
confirm DuckDB's dedup naming surfaces: `id, name, column2, name_1, notes` (empty → positional
`column2`; duplicate `name` → `name_1`), matching the Q3 resolution in `dev/DECISIONS.md`.

> Headless verification done: load, 14-column type sniff, the NULL counts, and the edge-cell parsing
> were confirmed via the `--output` path. The interactive **look** (alignment, the dim NULL glyph,
> ellipsis, sticky header, facet bars, popup glyphs) still needs a real-terminal eyeball.
