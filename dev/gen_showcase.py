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

Run:  python3 dev/gen_showcase.py   (writes tests/fixtures/showcase.csv)
"""

import csv
import os

ROWS = 5000  # > VIEWPORT_ROW_LIMIT (1000) so `SELECT * FROM t` triggers the truncation banner.

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


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(here, "..", "tests", "fixtures", "showcase.csv")
    out = os.path.normpath(out)
    # newline="" + the default (RFC-4180) dialect: embedded newlines/commas/quotes are quoted,
    # and the row terminator is CRLF per the spec — DuckDB's sniffer reads both fine.
    with open(out, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f, quoting=csv.QUOTE_MINIMAL)
        w.writerow(HEADER)
        for n in range(1, ROWS + 1):
            w.writerow(row(n))
    print(f"wrote {out} ({ROWS} data rows, {len(HEADER)} columns)")


if __name__ == "__main__":
    main()
