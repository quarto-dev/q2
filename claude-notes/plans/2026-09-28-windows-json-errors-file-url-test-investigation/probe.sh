#!/usr/bin/env bash
# Probe: does q2 emit Windows verbatim (\?\) paths when run from a plain cwd?
# Usage (Windows, Git Bash): cd to a scratch dir containing broken.ipynb + ok.qmd, then
#   bash probe.sh <path-to-q2.exe>
set -u
Q2="$1"
"$Q2" render --json-errors broken.ipynb 2> ipynb.stderr >/dev/null
echo "ipynb JSON path fields:"
grep '^{' ipynb.stderr | jq -c '.. | objects | with_entries(select(.key|test("path|file|notebook";"i"))) | select(length>0)' | sort -u
"$Q2" render ok.qmd 2>&1 | head -3
