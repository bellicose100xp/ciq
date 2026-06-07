# ciq — Human validation smoke script

The headless suite (`cargo test --all-features -- --test-threads=1`) proves all logic and the
*logical* cell grid (`TestBackend`). This file lists the small residue that only a real terminal can
confirm — the canonical §4.7 human surface. Per the plan these checks **batch into the P4/P5 gate**;
they are not separate blocking stops as each phase lands.

Run with a released/`cargo run --release -- <file.csv>` build against a CSV that has a
low-cardinality text column (e.g. `status`) and a date column, in **both** a light and a dark
terminal.

## Phase 3 — autocomplete popup (P3.6 / P3.7)

The headless snapshot proves the popup's logical cells only (which glyphs / candidates / the
right-aligned type-hint land where). It does NOT prove real glyphs, on-screen placement, or color
polarity. Confirm by hand:

1. **Popup opens + column candidates.** Type `SELECT st`. A popup appears under the query bar
   listing columns matching `st` (e.g. `status`), each with its type badge right-aligned (`txt`,
   `int`, `date`, …). Confirm the badge column is legible (not clipped, readable color).
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
   open. Confirm the popup stays anchored under the query bar, does not overflow the screen edge,
   and does not corrupt the grid behind it.
8. **Color polarity.** Repeat 1 and 6 in a light terminal and a dark terminal. Confirm the popup
   border, the selected-row highlight, and the dimmed type-hint column are all legible in both
   (the §4.7 polarity check).

## Phase 4 — schema bar (P4.1)

The headless snapshot proves the schema bar's logical cells (the `name (badge)` entries, their
alignment to the grid columns, the summary text). It does NOT prove the drawn underline on the
active column or the literal delimiter glyph as a real terminal renders them. Confirm by hand:

1. **Bar shows + aligns.** Run a query that returns a grid. Above the grid header sits a row of
   `name (badge)` labels (e.g. `id (int)   name (txt)   amount (num)`), each sitting dead-on over
   its data column. Scroll the grid horizontally (Right/Left while the grid has focus) and confirm
   the bar scrolls in lockstep (same columns drop off the left as the grid's).
2. **Delimiter/header summary.** The pane border title reads `delim , | header on` (or the actual
   delimiter for your file; a TSV shows `delim \t`). Confirm it is legible in a light and a dark
   terminal (the §4.7 polarity check).

## Phase 4 — column palette (P4.2-P4.5)

The headless suite proves the palette's generated SQL (`emit` goldens for both quoting surfaces),
the toggle/reorder/filter/ownership state machine, the ownership byte-compare, and the popup's
*logical* cells (80x24 `TestBackend` snapshot — which checkboxes / column names / right-aligned type
badges land where). It does NOT prove the drawn popup glyphs, the real `Space`/arrow chords as the
terminal delivers them, or the Replace-transition feel. Confirm by hand (open a CSV with a few
typed columns including a reserved-word column if you can, e.g. `order`):

1. **Open + checkboxes.** Press `Ctrl+K`. A bordered "columns" popup appears under the query bar
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
