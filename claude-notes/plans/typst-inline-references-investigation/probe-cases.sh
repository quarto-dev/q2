#!/usr/bin/env bash
# Mixed-document, margin and book cases for bd-yjf99rzx (see the plan's
# "Quarto 1 behavior" tables). Needs `q2`, Quarto 1 `quarto` and `typst` on
# PATH. Run from this directory: ./probe-cases.sh > probe-cases.txt 2>&1
set -uo pipefail
cd "$(dirname "$0")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

result() { # dir, command...
  local dir=$1; shift
  if (cd "$dir" && "$@" > out.log 2>&1); then echo OK
  else echo "FAIL ($(sed 's/\x1b\[[0-9;]*m//g' "$dir/out.log" | grep -m1 -o 'label `<[^>]*>` does not exist' || echo 'other error'))"; fi
}

cp -R mixed-cases "$work/mixed"
for f in uncited inboth inlineonly nocite; do
  echo "mixed/$f: q2=$(result "$work/mixed" q2 render $f.qmd --to typst) q1=$(result "$work/mixed" quarto render $f.qmd --to typst)"
done

cp single-margin.qmd "$work/"
echo "single margin: q2=$(result "$work" q2 render single-margin.qmd --to typst) q1=$(result "$work" quarto render single-margin.qmd --to typst)"

for mode in plain margin; do
  mkdir -p "$work/book-$mode"
  cp book/* "$work/book-$mode/"
  [ "$mode" = margin ] && echo "citation-location: margin" >> "$work/book-$mode/_quarto.yml"
  cp -R "$work/book-$mode" "$work/book-$mode-q1"
  echo "book $mode: q2=$(result "$work/book-$mode" q2 render) q1=$(result "$work/book-$mode-q1" quarto render)"
done
