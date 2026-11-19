#!/usr/bin/env bash
# Lists each run.ps1 item's nextest filter on this host and fails when it matches fewer than its min_tests.
# macOS/Linux only (temporary tooling, never lands). Passes each item's -p packages so nextest builds only those
# crates. Run from the repository root:
#   scripts/git-epochs-windows/check_filters.sh
set -u
here="$(cd "$(dirname "$0")" && pwd)"
status=0
n=$(jq length "$here/items.json")
for i in $(seq 0 $((n - 1))); do
  id=$(jq -r ".[$i].id" "$here/items.json")
  filter=$(jq -r ".[$i].filter" "$here/items.json")
  min=$(jq -r ".[$i].min_tests" "$here/items.json")
  pkgs=$(jq -r ".[$i].packages" "$here/items.json")
  count=$(cargo nextest list $pkgs -E "$filter" --message-format oneline 2>/dev/null | wc -l | tr -d ' ')
  if [ "$count" -ge "$min" ]; then tag=ok; else tag=TOO-FEW; status=1; fi
  printf '%-4s %-8s listed=%-4s min=%-4s %s\n' "$id" "$tag" "$count" "$min" "$filter"
done
exit $status
