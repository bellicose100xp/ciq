#!/usr/bin/env python3
"""Deterministic generator for tests/fixtures/showcase.csv.

Every value is derived purely from the 1-based row index, so the file is reproducible
and trivially extendable (change ROWS, re-run). No randomness, no external deps.

The fixture is designed so a human can drive every edge case ciq's TUI handles:
type sniffing + per-type alignment, NULL (empty-field) rendering + Q12 semantics,
vertical scroll + the 1000-row truncation banner, wide-column ellipsis + horizontal
column scroll, RFC-4180 quoting (embedded comma / doubled-quote / embedded newline),
multibyte/wide-glyph truncation safety, low-cardinality facets + value completion, and
identifier quoting for awkward column names.

Output modes:
  python3 dev/gen_showcase.py            -> both manual-test files (repo root, gitignored via
                                            the /*.csv entry; never commit them):
                                              showcase-xl.csv  1,000,000 rows x 100 columns (~800 MB)
                                              showcase-m.csv     100,000 rows x  20 columns (~25 MB)
  python3 dev/gen_showcase.py xl|m       -> just that one manual-test file
  python3 dev/gen_showcase.py --fixture  -> tests/fixtures/showcase.csv (tracked):
                                            5,000 rows x 14 columns — the small edge-case fixture
                                            documented in dev/human-smoke.md.

The manual-test files keep the same first 14 edge-case columns, then append deterministic filler
columns (c015..) cycling through int / float / short-text / bool / date so type sniffing,
alignment, and horizontal scroll all have real work at width.
"""

import csv
import os
import sys

FIXTURE_ROWS = 5000  # comfortably above any configured row_limit so a cap is exercisable.

# The manual-test size tiers: name -> (rows, total columns incl. the 14 edge-case columns).
SIZES = {
    "xl": (1_000_000, 100),
    "m": (100_000, 20),
}

# Column names chosen to exercise identifier quoting on emit:
#   "Total ($)" -> space + special chars   |   "order" -> reserved word   |   "CreatedBy" -> CamelCase
HEADER = [
    "id", "name", "region", "status", "amount", "quantity", "active",
    "created_at", "updated_at", "score", "notes", "Total ($)", "order", "CreatedBy",
]

NAMES = [
    "Ada", "Babbage", "Curie", "Dijkstra", "Euler", "Fermat", "Gauss", "Hopper",
    "Iverson", "Jacobi", "Knuth", "Lovelace", "Mandel", "Newton", "Oresme",
    "Pascal", "Quine", "Riemann", "Shannon", "Turing", "Ulam", "Venn", "Wiener",
    "Xenakis", "Yates", "Zorn", "Archimedes", "Boole", "Cauchy", "Descartes",
]
REGIONS = ["EU", "NA", "APAC", "LATAM", "MEA"]
STATUSES = ["active", "inactive", "pending", "churned"]
CREATORS = ["alice", "Bob", "Carol", "Dave", "Eve"]  # mixed-case -> case-preservation quoting

# Plain note sentences (60-120 chars) — the bulk of the `notes` column. The wide content forces
# ellipsis truncation and horizontal column scroll.
PLAIN_NOTES = [
    "Routine quarterly review completed with no outstanding action items recorded.",
    "Customer requested an extended trial; follow up before the end of the billing cycle.",
    "Migration to the new pipeline finished ahead of schedule and within the agreed budget.",
    "Flagged for manual audit after an unusually large transaction was observed last week.",
    "Onboarding paperwork is complete and the account has been provisioned successfully.",
    "Pending verification of the secondary contact before the upgrade can be applied here.",
    "Renewal confirmed for another twelve months at the standard enterprise support tier.",
    "Escalated to the regional team because the SLA window was missed twice this quarter.",
    "No activity recorded for ninety days; consider a re-engagement campaign next month.",
    "Resolved the duplicate-record issue and merged the two profiles into a single entry.",
]

# Special cells, keyed by (row_index % 250). Each lands roughly every 250 rows so they are
# findable but the bulk stays normal. These exercise RFC-4180 + unicode edge cases; Python's
# csv writer handles the quoting (commas, doubled quotes, embedded newlines) per RFC-4180.
SPECIAL_NOTES = {
    0: "Smith, Jr., flagged for review",                 # embedded commas -> quoted field
    50: 'he said ""ship it"" today',                      # literal text w/ doubled quotes (see below)
    100: "line one\nline two",                            # embedded newline -> quoted, spans lines
    150: "café au lait, naïve façade, Zürich branch",     # latin-1 multibyte + a comma
    200: "北京 office — 日本語メモ included for review",     # CJK wide glyphs + em dash
}
# A second band for the remaining unicode/emoji cases (every 250 rows, offset by 25).
SPECIAL_NOTES_B = {
    25: "¡Hola! naïve café update from the LATAM team",   # leading inverted punctuation
    125: "launch 🎉 review scheduled with the whole crew", # emoji (wide/ZWJ-free)
    225: "emoji set 😀🚀✅ appended to the status summary", # multiple emoji
}


def q(n: int) -> str:
    """Deterministic note for row n (1-based)."""
    m = n % 250
    if m in SPECIAL_NOTES:
        # The doubled-quote case: store the intended *text* with real double quotes so the csv
        # writer escapes them per RFC-4180 (each " -> "").
        if m == 50:
            return 'he said "ship it" today'
        return SPECIAL_NOTES[m]
    if m in SPECIAL_NOTES_B:
        return SPECIAL_NOTES_B[m]
    return PLAIN_NOTES[n % len(PLAIN_NOTES)]


def row(n: int):
    """Build row n (1-based). Empties are real empty strings -> unquoted empty fields -> SQL NULL."""
    # --- nullable fields (empty -> NULL). Different cadences so combos appear. ---
    status = "" if n % 17 == 0 else STATUSES[n % len(STATUSES)]
    amount = "" if n % 23 == 0 else f"{(((n * 37) % 10000) - 2000) + (n % 100) / 100:.2f}"
    # A few very large amounts to test wide right-aligned numerics.
    if n % 500 == 0:
        amount = f"{9_000_000 + n}.99"
    quantity = "" if n % 19 == 0 else str((n * 7) % 500)
    active = "" if n % 29 == 0 else ("true" if (n % 2 == 0) else "false")

    # --- always-present typed fields ---
    # created_at: DATE spread across 2019..2024.
    year = 2019 + (n % 6)
    month = (n % 12) + 1
    day = (n % 28) + 1
    created_at = f"{year:04d}-{month:02d}-{day:02d}"
    # updated_at: TIMESTAMP, same date a bit later in the day.
    hh = (n * 3) % 24
    mm = (n * 7) % 60
    ss = (n * 11) % 60
    updated_at = f"{created_at} {hh:02d}:{mm:02d}:{ss:02d}"

    # score: higher-precision DOUBLE with a couple of deliberate edge values.
    if n % 311 == 0:
        score = "0.0"
    elif n % 313 == 0:
        score = "0.0001"
    else:
        score = f"{((n * 911) % 100000) / 1000:.4f}"

    total = f"{(((n * 53) % 50000) / 100):.2f}"  # "Total ($)" -> DOUBLE
    order_val = str(n % 50)                       # "order" -> small INT
    creator = CREATORS[n % len(CREATORS)]

    return [
        str(n),                       # id (INTEGER)
        NAMES[n % len(NAMES)],        # name (TEXT)
        REGIONS[n % len(REGIONS)],    # region (low-card TEXT)
        status,                       # status (low-card TEXT, some NULL)
        amount,                       # amount (DOUBLE, some NULL, neg + large)
        quantity,                     # quantity (INTEGER, some NULL)
        active,                       # active (BOOLEAN, some NULL)
        created_at,                   # created_at (DATE)
        updated_at,                   # updated_at (TIMESTAMP)
        score,                        # score (DOUBLE)
        q(n),                         # notes (wide TEXT + edge cases)
        total,                        # "Total ($)" (DOUBLE)
        order_val,                    # "order" (INTEGER)
        creator,                      # CreatedBy (CamelCase TEXT)
    ]


def filler_header(total_cols: int):
    """Names for the deterministic filler columns c015..c<total_cols>."""
    return [f"c{i:03d}" for i in range(len(HEADER) + 1, total_cols + 1)]


def filler_cells(n: int, total_cols: int):
    """Filler values for row n: cycle int / float / short-text / bool / date by column index so
    the wide file exercises sniffing + alignment across every column. Pure f(n, i)."""
    cells = []
    for i in range(len(HEADER) + 1, total_cols + 1):
        k = (n * 31 + i * 17) % 100000
        kind = i % 5
        if kind == 0:
            cells.append(str(k))
        elif kind == 1:
            cells.append(f"{k / 100:.2f}")
        elif kind == 2:
            cells.append(NAMES[k % len(NAMES)])
        elif kind == 3:
            cells.append("true" if k % 2 == 0 else "false")
        else:
            cells.append(f"{2019 + k % 6:04d}-{k % 12 + 1:02d}-{k % 28 + 1:02d}")
    return cells


def write_csv(out: str, rows: int, total_cols: int | None):
    """Write `rows` data rows to `out`; `None` total_cols means the bare 14-column fixture.

    newline="" + the default (RFC-4180) dialect: embedded newlines/commas/quotes are quoted,
    and the row terminator is CRLF per the spec — DuckDB's sniffer reads both fine.
    """
    header = HEADER if total_cols is None else HEADER + filler_header(total_cols)
    with open(out, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f, quoting=csv.QUOTE_MINIMAL)
        w.writerow(header)
        for n in range(1, rows + 1):
            r = row(n)
            if total_cols is not None:
                r += filler_cells(n, total_cols)
            w.writerow(r)
            if n % 100_000 == 0:
                print(f"  {n:,} rows...", flush=True)
    print(f"wrote {out} ({rows:,} data rows, {len(header)} columns)")


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    root = os.path.normpath(os.path.join(here, ".."))
    if "--fixture" in sys.argv:
        write_csv(os.path.join(root, "tests", "fixtures", "showcase.csv"), FIXTURE_ROWS, None)
        return
    picked = [a for a in sys.argv[1:] if a in SIZES] or list(SIZES)
    for name in picked:
        rows, cols = SIZES[name]
        write_csv(os.path.join(root, f"showcase-{name}.csv"), rows, cols)


if __name__ == "__main__":
    main()
