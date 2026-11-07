#!/usr/bin/env bash
# EXR-8 B3: the TS presenter's markdown for each parity fixture notebook.
#
# Renders every crates/quarto-core/tests/fixtures/julia-presenter-parity/<case>.ipynb with
# the local `quarto` (the TS `jupyterToMarkdown` path) and keeps the post-engine markdown
# (`-M keep-md:true`) as <case>.ts.md beside it, front matter removed.
#
#   generate-ts-goldens.sh           write the goldens
#   generate-ts-goldens.sh --check   fail if a committed golden differs (freshness check)
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
dir="$root/crates/quarto-core/tests/fixtures/julia-presenter-parity"
mode="write"
[ "${1:-}" = "--check" ] && mode="check"
command -v quarto >/dev/null || { echo "quarto is not on PATH" >&2; exit 2; }

status=0
for nb in "$dir"/*.ipynb; do
  case="$(basename "$nb" .ipynb)"
  work="$(mktemp -d)"
  # Render under the name the Rust test uses so figure directories compare equal.
  cp "$nb" "$work/notebook.ipynb"
  (cd "$work" && quarto render notebook.ipynb --to html -M keep-md:true >/dev/null 2>&1) \
    || { echo "quarto render failed for $case" >&2; status=1; rm -rf "$work"; continue; }
  out="$work/ts.md"
  awk 'BEGIN{fm=0} NR==1&&/^---$/{fm=1;next} fm&&/^---$/{fm=0;skip=1;next} fm{next} skip&&/^$/{skip=0;next} {skip=0;print}' \
    "$work/notebook.html.md" > "$out"
  if [ "$mode" = "write" ]; then
    cp "$out" "$dir/$case.ts.md"
  elif ! diff -u "$dir/$case.ts.md" "$out" >/dev/null; then
    echo "stale golden: $case.ts.md" >&2
    status=1
  fi
  rm -rf "$work"
done
exit $status
