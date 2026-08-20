#!/usr/bin/env bash
# Print the q2-vs-pandoc parity matrix for Lua filter script return values
# (bd-lua-filter-table-form-ignored-ph23becz). Run from this directory.
#
# Compares the same (input.md, probe.lua) pair through both engines, the way
# tests/lua-conformance/differential does:
#
#   pampa input.md -F probe.lua -t plain
#   pandoc -f markdown input.md -L probe.lua -t plain
#
# PAMPA defaults to the workspace debug build; override to test a fix:
#   PAMPA=target/release/pampa ./run-parity-matrix.sh
set -uo pipefail
cd "$(dirname "$0")"

PAMPA=${PAMPA:-../../../../target/debug/pampa}
[ -x "$PAMPA" ] || { echo "no pampa at $PAMPA (cargo build -p pampa)" >&2; exit 1; }

printf '%-10s | %-24s | %-24s\n' probe 'pandoc' 'q2'
printf -- '-----------+--------------------------+-------------------------\n'
for f in tf lf hybrid trav mixed empty nilret num fnret emptylist listtrav; do
  [ -f "$f.lua" ] || continue
  p=$(pandoc -f markdown input.md -L "$f.lua" -t plain 2>&1 | head -1)
  q=$("$PAMPA" input.md -F "$f.lua" -t plain 2>&1 | head -1)
  printf '%-10s | %-24s | %-24s\n' "$f" "${p:0:24}" "${q:0:24}"
done
