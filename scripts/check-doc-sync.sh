#!/usr/bin/env bash
# Verify that each English canonical document and its Japanese counterpart
# (*.md / *.ja.md) have the same number of headings (AGENTS.md rule: keep both in sync).
# Lines inside fenced code blocks are ignored.
set -euo pipefail
cd "$(dirname "$0")/.."

count_headings() {
    awk '/^```/ { in_code = !in_code; next } !in_code && /^#+ / { n++ } END { print n + 0 }' "$1"
}

status=0
for en in README.md architecture.md ROADMAP.md TESTING.md CHANGELOG.md CONTRIBUTING.md; do
    ja="${en%.md}.ja.md"
    if [[ ! -f "$en" || ! -f "$ja" ]]; then
        echo "MISSING  $en / $ja"
        status=1
        continue
    fi
    n_en=$(count_headings "$en")
    n_ja=$(count_headings "$ja")
    if [[ "$n_en" == "$n_ja" ]]; then
        echo "OK       $en ($n_en) == $ja ($n_ja)"
    else
        echo "MISMATCH $en ($n_en) != $ja ($n_ja)"
        status=1
    fi
done
exit $status
