#!/usr/bin/env bash
# Regenerate the committed oracle snapshots for the Lua differential
# conformance suite (Track 2 of bd-grkrb9nj; see ../README.md).
#
# The oracle is a real `pandoc` binary, pinned to the version recorded
# in ORACLE_VERSION. CI never runs pandoc — it compares against these
# committed snapshots — so this script is a local dev step, needed only
# when cases change or the pinned oracle version is deliberately bumped
# (bump = edit ORACLE_VERSION + rerun + review every snapshot diff).
set -euo pipefail
cd "$(dirname "$0")"

want=$(cat ORACLE_VERSION)
have=$(pandoc --version | head -1 | awk '{print $2}')
if [ "$have" != "$want" ]; then
  echo "error: oracle is pinned to pandoc $want, but 'pandoc' on PATH is $have" >&2
  echo "       (install the pinned version, or deliberately bump ORACLE_VERSION)" >&2
  exit 1
fi

for dir in cases/*/; do
  name=$(basename "$dir")
  pandoc -f markdown "$dir/input.md" -L "$dir/filter.lua" -t json \
    | jq -S . > "$dir/oracle.json"
  echo "regenerated $name"
done
echo "done (pandoc $have)"
