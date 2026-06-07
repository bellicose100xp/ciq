#!/usr/bin/env bash
# Shell-containment HARD gate (dev/PLAN.md §0/D5, §4.7).
#
# Fails if a `// ciq:shell-exempt` marker appears on any file NOT in dev/shell-exempt.txt.
# This keeps the human-validated surface frozen — it cannot silently grow as features land.
set -euo pipefail

cd "$(dirname "$0")/../.."   # repo root

ALLOWLIST="dev/shell-exempt.txt"
MARKER="ciq:shell-exempt"

# Allowed paths (strip comments/blanks).
allowed="$(grep -vE '^\s*(#|$)' "$ALLOWLIST" 2>/dev/null || true)"

# Files carrying the marker (search src/; rg if present, else grep).
if command -v rg >/dev/null 2>&1; then
    marked="$(rg -l --no-messages "$MARKER" src/ 2>/dev/null || true)"
else
    marked="$(grep -rlE "$MARKER" src/ 2>/dev/null || true)"
fi

violations=()
while IFS= read -r f; do
    [ -z "$f" ] && continue
    if ! printf '%s\n' "$allowed" | grep -qxF "$f"; then
        violations+=("$f")
    fi
done <<< "$marked"

if [ "${#violations[@]}" -gt 0 ]; then
    echo "FAIL: '// $MARKER' marker found on file(s) not in $ALLOWLIST:" >&2
    printf '  %s\n' "${violations[@]}" >&2
    echo "" >&2
    echo "The human-validated shell surface (§4.7) must not grow. Either extract the pure" >&2
    echo "core out of these files, or (if it genuinely belongs on the §4.7 list) add it to" >&2
    echo "$ALLOWLIST with the §4.7 row it corresponds to." >&2
    exit 1
fi

echo "OK: shell-exempt markers stay within the §4.7 allowlist."
