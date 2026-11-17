#!/usr/bin/env bash
# Round-trip probe for bd-2bcre8cr: read each case with pampa, write it back
# as qmd, re-read, and compare pampa's native AST before and after. Also shows
# pandoc -f commonmark on the *written* text as a CommonMark reference.
# Usage: roundtrip.sh [path/to/pampa]   (default: target/debug/pampa)
set -u
P=${1:-target/debug/pampa}
tmp=$(mktemp -d)
cases=(
  '- 1.\n  1)\n'
  '- 1. a\n  1) b\n'
  '- x\n  1. a\n  1)\n'
  '- x\n  1. a\n  1. b\n'
  '1.\n1)\n'
  '1. a\n\n<!-- -->\n\n1.\n'
  '- 1. a\n\n  <!-- -->\n\n  1.\n'
  'a\n<!-- c -->\nb\n'
)
for c in "${cases[@]}"; do
  printf -- "$c" > "$tmp/in.qmd"
  "$P" -t native "$tmp/in.qmd" 2>/dev/null | tr -s ' \n' ' ' > "$tmp/before"
  "$P" -t qmd "$tmp/in.qmd" 2>/dev/null > "$tmp/written.qmd"
  "$P" -t native "$tmp/written.qmd" 2>/dev/null | tr -s ' \n' ' ' > "$tmp/after"
  echo "=== input:   $(printf '%q' "$(printf -- "$c")")"
  echo "    before:  $(cat "$tmp/before")"
  echo "    written: $(printf '%q' "$(cat "$tmp/written.qmd")")"
  echo "    after:   $(cat "$tmp/after")"
  if cmp -s "$tmp/before" "$tmp/after"; then echo "    ROUND TRIP: same"; else echo "    ROUND TRIP: DIFFERS"; fi
done
rm -rf "$tmp"
