#!/usr/bin/env bash
# Pure-core coverage FLOOR — HARD gate (dev/PLAN.md §0/D5).
#
# Fails the build if any module in dev/core-modules.txt falls below the floor. The pure core
# can't be coverage-padded (every branch is a real behavior test), so a hard floor here is
# gaming-free and catches the most dangerous regressions. Usage:
#   core-floor.sh <cobertura.xml> [floor_pct]
#
# NOTE on branch vs line (D5 risk #1): cobertura from tarpaulin reports per-class line-rate
# reliably; its branch-rate support is historically weak. This script gates on LINE rate and
# treats the floor conservatively. If/when tarpaulin branch data is trustworthy, switch the
# parsed attribute to branch-rate. Until then the line floor is set high (matching the intent
# that pure modules are ~fully covered).
set -euo pipefail

cd "$(dirname "$0")/../.."   # repo root

REPORT="${1:-coverage/cobertura.xml}"
FLOOR="${2:-95}"
ALLOWLIST="dev/core-modules.txt"

if [ ! -f "$REPORT" ]; then
    echo "FAIL: coverage report not found at $REPORT" >&2
    exit 1
fi

mapfile -t cores < <(grep -vE '^\s*(#|$)' "$ALLOWLIST" 2>/dev/null || true)
if [ "${#cores[@]}" -eq 0 ]; then
    echo "OK: no pure-core modules listed yet — floor vacuously satisfied."
    exit 0
fi

fail=0
checked=0
for path in "${cores[@]}"; do
    # Cobertura <class filename="src/schema/types.rs" line-rate="0.97" ...>
    line="$(grep -oE "<class[^>]*filename=\"${path//\//\\/}\"[^>]*line-rate=\"[0-9.]+\"" "$REPORT" | head -1 || true)"
    if [ -z "$line" ]; then
        # Module present in allowlist but absent from report — likely no executable lines yet
        # (pure type defs) or filename mismatch. Warn, don't hard-fail on absence.
        echo "::warning::core module '$path' not found in coverage report (no executable lines yet?)"
        continue
    fi
    rate="$(printf '%s' "$line" | grep -oE 'line-rate="[0-9.]+"' | grep -oE '[0-9.]+')"
    pct="$(awk -v r="$rate" 'BEGIN { printf "%.1f", r * 100 }')"
    below="$(awk -v p="$pct" -v f="$FLOOR" 'BEGIN { print (p < f) ? 1 : 0 }')"
    checked=$((checked + 1))
    if [ "$below" -eq 1 ]; then
        echo "FAIL: core module '$path' line coverage ${pct}% < floor ${FLOOR}%" >&2
        fail=1
    else
        echo "ok: '$path' ${pct}% >= ${FLOOR}%"
    fi
done

if [ "$fail" -eq 1 ]; then
    echo "" >&2
    echo "Pure-core modules must meet the ${FLOOR}% floor (D5). These are data-in/data-out;" >&2
    echo "cover the uncovered branch with a real behavior test." >&2
    exit 1
fi

echo "OK: ${checked} pure-core module(s) meet the ${FLOOR}% floor."
