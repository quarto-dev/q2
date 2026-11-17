#!/usr/bin/env bash
# Probe Pandoc's citeproc locale precedence: document `lang` vs CSL default-locale.
# Run from this directory: ./pandoc-probe.sh > pandoc-probe.txt
set -euo pipefail
cd "$(dirname "$0")"
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
for L in nl nl-NL en de ""; do
  for C in "" style-de.csl; do
    if [ -z "$L" ]; then grep -v '^lang:' repro.qmd > "$tmp"; else sed "s/^lang: nl$/lang: $L/" repro.qmd > "$tmp"; fi
    args=(--citeproc -f markdown -t plain); [ -n "$C" ] && args+=(--csl "$C")
    echo "lang=[$L] csl=[${C:-default}]: $(pandoc "$tmp" "${args[@]}" | tr '\n' ' ')"
  done
done
