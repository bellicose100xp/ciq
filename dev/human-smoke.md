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
