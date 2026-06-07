#!/usr/bin/env bash
# Project-wide coverage WARN gate (dev/PLAN.md §0/D5).
#
# Annotates a warning if overall line coverage is below the target, but NEVER fails the build
# (a blunt project-wide percentage trains test-padding; the hard floor is on the pure core
# only — see core-floor.sh). Usage: coverage-warn.sh <target_pct> <cobertura.xml>
set -euo pipefail

TARGET="${1:-95}"
REPORT="${2:-coverage/cobertura.xml}"

if [ ! -f "$REPORT" ]; then
    echo "::warning::coverage report not found at $REPORT — skipping project-wide warn check"
    exit 0
fi

# Cobertura puts overall line-rate on the root <coverage line-rate="0.93..."> attribute.
rate="$(grep -oE 'line-rate="[0-9.]+"' "$REPORT" | head -1 | grep -oE '[0-9.]+' || true)"
if [ -z "$rate" ]; then
    echo "::warning::could not parse line-rate from $REPORT — skipping"
    exit 0
fi

pct="$(awk -v r="$rate" 'BEGIN { printf "%.1f", r * 100 }')"
below="$(awk -v p="$pct" -v t="$TARGET" 'BEGIN { print (p < t) ? 1 : 0 }')"

if [ "$below" -eq 1 ]; then
    echo "::warning::project-wide coverage ${pct}% is below the ${TARGET}% target (warn-only, build passes)"
else
    echo "project-wide coverage ${pct}% meets the ${TARGET}% target"
fi
exit 0
