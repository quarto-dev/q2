#!/usr/bin/env bash
# EXR-1: the presenter goldens are generated once, from the unmodified tree, and never
# change afterwards. Fails if anything under the goldens directory (README excepted)
# differs from the commit that first added it.
set -euo pipefail

dir=crates/quarto-core/tests/fixtures/presenter-goldens
first=$(git log --diff-filter=A --reverse --format=%H -- "$dir/README.md" | head -n 1)
if [ -z "$first" ]; then
  echo "check-presenter-goldens-unchanged: no commit adds $dir/README.md yet" >&2
  exit 1
fi
git diff --exit-code "$first" -- "$dir" ":(exclude)$dir/README.md"
echo "presenter goldens unchanged since ${first:0:9}"
