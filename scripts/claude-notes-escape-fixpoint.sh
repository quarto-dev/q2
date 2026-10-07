#!/bin/bash
# Run every reviewed q2-escape-openers.py rule over claude-notes until none
# changes anything (bd-uk8zgkha). Fixing one class lets the parser reach
# further, which exposes more of the others, so they must be iterated jointly.
# Star rules escape only clearly literal stars; the rest need manual review.
cd "$(git rev-parse --show-toplevel)"
# Clearly literal stars: whitespace-flanked, glob (/* */ *.rs), trailing
# wildcard (Q-2-*, epub_*, website.*), raw pointer (*const), a star before a
# digit ((*50.6), and the keycap emoji *⃣.
STAR='\*{1,2}(?=\s)|(?<=/)\*|\*{1,2}(?=/)|\*(?=\.\w)|(?<=[\w.-])\*(?=[\s),;:\]]|$)|(?<=<)\*(?=const|mut)|\*(?=\d)|\*(?=\uFE0F)'
# An apostrophe after a span closer: when a star is not clearly literal, this
# is the usual real culprit in the same block (`x`'s inside **bold**).
APOS="(?<=[\`*_~^\\]])'(?=\\w)"
run() { scripts/q2-escape-openers.py claude-notes "$@" --max-rounds 20 2>&1 | awk '/^total/{print $3}'; }
for i in 1 2 3 4 5 6 7 8; do
  a=$(run Q-2-17 --block-pattern '~')
  b=$(run Q-2-7 --block-pattern "(?<=[\`*_~^\\]])'(?=\\w)")
  c=$(run Q-2-10 --only-context "(?<=[\\w\`])'")
  d=$(run bare-@)
  e=$(run Q-2-12 --only-context "$STAR" --block-pattern "$APOS")
  f=$(run Q-2-13 --only-context "$STAR" --block-pattern "$APOS")
  echo "pass $i: Q-2-17+$a Q-2-7+$b Q-2-10+$c @+$d Q-2-12+$e Q-2-13+$f"
  [ $((a+b+c+d+e+f)) = 0 ] && break
done
