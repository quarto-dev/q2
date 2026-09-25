#!/usr/bin/env bash
# Runs every case in ./cases through the tree-sitter CLI against the
# built qmd grammar and prints ok/ERROR per case plus the tree.
# Usage (from repo root): bash claude-notes/plans/code-span-backtick-run-investigation/run-cases.sh
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
cd "$ROOT/crates/tree-sitter-qmd/tree-sitter-markdown" || exit 1
for f in "$HERE"/cases/*.md; do
  name="$(basename "$f" .md)"
  out="$(tree-sitter parse "$f" 2>/dev/null | grep -v '^$' | grep -v 'Parse:')"
  if printf '%s' "$out" | grep -q 'ERROR'; then status=ERROR; else status=ok; fi
  printf '=== %-36s %-5s | %s\n' "$name" "$status" "$(tr '\n' '|' < "$f")"
  printf '%s\n' "$out" | sed 's/^/    /'
done
