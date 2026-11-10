#!/usr/bin/env bash
# Reproduce the findings in ../2026-11-07-typst-inline-references.md.
# Needs `q2` and Quarto 1 `quarto` on PATH, plus `typst`. Run from this directory:
#   ./probe.sh > probe.txt 2>&1
set -uo pipefail
cd "$(dirname "$0")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp repro.qmd mixed.qmd refs.bib "$work"/

run() { # label, command...
  local label=$1; shift
  if (cd "$work" && "$@" > out.log 2>&1); then echo "$label: OK"
  else echo "$label: FAIL: $(sed 's/\x1b\[[0-9;]*m//g' "$work/out.log" | grep -m1 'label `')"; fi
}

echo "q2 $(q2 --version 2>&1 | head -1); quarto $(quarto --version); $(typst --version)"
run "q2  repro (references only)" q2 render repro.qmd --to typst
run "q1  repro (references only)" quarto render repro.qmd --to typst
run "q2  mixed (bibliography + references)" q2 render mixed.qmd --to typst
run "q1  mixed (bibliography + references)" quarto render mixed.qmd --to typst
sed 's/^references:/citeproc: true\nreferences:/' repro.qmd > "$work/cp.qmd"
run "q2  repro + citeproc: true" q2 render cp.qmd --to typst
run "q1  repro + citeproc: true" quarto render cp.qmd --to typst

# Which bibliography formats Typst's native #bibliography() reads.
for f in refs.json refs.yml refs.bib; do
  cp "typst-bib-formats/$f" "$work/"
  printf 'See @item.\n#bibliography("%s")\n' "$f" > "$work/t.typ"
  if (cd "$work" && typst compile t.typ t.pdf > t.log 2>&1); then echo "typst #bibliography($f): OK"
  else echo "typst #bibliography($f): FAIL: $(grep -m1 error "$work/t.log")"; fi
done
