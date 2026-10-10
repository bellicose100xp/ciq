---
title: Vim editing
layout: default
parent: Features
nav_order: 10
---

# Vim editing

Every query box in ciq is a vim-style modal editor: the free-form SQL box in Power mode and each
of the five clause panes (`SELECT`, `WHERE`, `GROUP BY`, `ORDER BY`, `LIMIT`) in Simple mode. They
share one implementation, so the same motions, operators, text objects, and register work the same
way wherever the cursor is.

A box starts in **INSERT** mode, so typing works with no vim knowledge. Press `Esc` to drop to
**NORMAL** mode for navigation and edits; `i`, `a`, `o`, and the other usual entries take you back.
The current mode rides the query box's top border as a badge, and the border, the badge, and the
cursor block take the mode's color: cyan for Insert, yellow for Normal, green while an operator is
pending, pink while a character search is pending.

## The five panes are five lines

In Simple mode the form behaves like one editor whose lines are the panes. In Normal mode `j` and
`k` (or `Down`, `Up`, and `Enter`) step to the next or previous pane, `gg` jumps to `SELECT`, and
`G` jumps to `LIMIT`. The mode travels with the focus, so you stay in Normal mode as you move, and
so does the register, so a `yy` in `WHERE` followed by `j` and `p` lands the text in `GROUP BY`.
The same carry runs across `Ctrl+Q`, so toggling to Power mode keeps the mode and the register you
had.

A pane can never grow a second row. `Enter` in Insert mode is a no-op there, `o` and `O` behave
like `A` and `I`, and a pasted or recalled clause that contains newlines is flattened to one line.
Power mode has none of these limits: `Enter` inserts a newline, `o` opens a line, and `j`/`k` walk
the lines of the query. `j` or `Enter` on the last line hands focus to the results grid, the same
hand-off the Insert-mode `Down` makes.

## Normal mode

| Key | Action |
|---|---|
| `h` / `l`, `Left` / `Right` | Move one character. |
| `w` / `b` / `e` | Next word start / previous word start / word end. |
| `0` / `^` / `$`, `Home` / `End` | Line start / line end. |
| `j` / `k`, `Down` / `Up`, `Enter` (as `j`) | Next / previous line. In Simple mode: next / previous pane. |
| `gg` / `G` | First / last line. In Simple mode: the `SELECT` / `LIMIT` pane. |
| `f<c>` / `F<c>` | Jump to the next / previous `<c>` on the line. |
| `t<c>` / `T<c>` | Jump to just before the next / just after the previous `<c>`. |
| `;` / `,` | Repeat the last character search / repeat it reversed. |
| `i` / `a` | Insert at / after the cursor. |
| `I` / `A` | Insert at the line start / line end. |
| `o` / `O` | Open a line below / above and insert (Power). Insert at the end / start of the pane (Simple). |
| `x` / `X` | Delete the character under / before the cursor. |
| `D` / `C` | Delete to the line end / delete to the line end and insert. |
| `dd` / `cc` | Delete the line / clear the line and insert. The text goes to the register. |
| `d<motion>` / `c<motion>` | Delete / change across `w`, `b`, `e`, `0`, `^`, `$`, `h`, `l`, `f<c>`, `t<c>`, `F<c>`, `T<c>`. |
| `diw` / `daw`, `di"` / `da"`, `di'`, `` di` ``, `di(` / `da(`, `di[`, `di{` | Delete inside / around a word or a delimited span. `c` instead of `d` changes it. |
| `yy` / `Y` | Yank the line into the register and copy it to the system clipboard. |
| `y<motion>`, `yiw`, `yi"`, `yf<c>`, … | Yank the span the way `d` would delete it. |
| `p` / `P` | Paste the register after / at the cursor. |
| `u` | Undo. |
| `/` | Open the row search over the results (see [Row search](search.md)). |
| `Esc` | Stay in Normal mode. Cancels a pending operator. |

Every delete and change also fills the register, so `dw` then `p` moves a word and `ci'` then
`p` recovers the old literal. The register holds text, not lines: a yanked line pastes inline at the
cursor rather than as a new row, which is what a one-line SQL clause needs. `Ctrl+R` is ciq's query
history, so there is no redo binding; `u` undoes.

## Insert mode

Insert mode is ordinary typing: printable characters insert, `Backspace` and `Delete` remove,
`Left`, `Right`, `Home`, and `End` move, a paste inserts the payload, and the autocomplete popup
works as usual. `Esc` drops to Normal mode and steps the cursor back one column, as vim does. When
the autocomplete popup is open, `Esc` closes the popup first and a second `Esc` changes mode. The
popup belongs to Insert mode: it stays closed in Normal mode, so `Enter`, `Up`, `Down`, and `Tab`
there are always motions, and it comes back with the next edit after `i`.

## Clipboard

A yank also writes the register to the system clipboard through the terminal's OSC 52 escape, so
`yy` in a query box leaves the clause ready to paste into another program. Deletes stay in the
in-app register only. A terminal that ignores OSC 52 still has `p`.
