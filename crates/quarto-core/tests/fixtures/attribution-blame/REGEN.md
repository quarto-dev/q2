# Regenerating attribution-blame porcelain fixtures

The `.porcelain` files in this directory were captured from a hand-built
git repository with deterministic author timestamps. They are checked in
verbatim so the parsing unit tests (Phase 0 test #3, Phase 0 test #12)
do not depend on live commit hashes or timestamps.

To regenerate the fixtures (e.g. after the porcelain parser learns a new
field), use the helper script below. Identities are pinned so the
serialized output is reproducible across machines:

```bash
set -euo pipefail
tmp=$(mktemp -d)
cd "$tmp"
git init -q -b main
export GIT_CONFIG_NOSYSTEM=1
export GIT_CONFIG_GLOBAL=/dev/null

# single-commit.porcelain
echo "hello" > doc.qmd
git add doc.qmd
GIT_AUTHOR_NAME=Alice GIT_AUTHOR_EMAIL=alice@example.com \
GIT_COMMITTER_NAME=Alice GIT_COMMITTER_EMAIL=alice@example.com \
GIT_AUTHOR_DATE="@1700000000 +0000" GIT_COMMITTER_DATE="@1700000000 +0000" \
  git -c commit.gpgsign=false commit -q -m initial
git blame --porcelain doc.qmd  # → single-commit.porcelain

# multi-commit.porcelain
printf 'line1\n世界\n' > doc.qmd
git add doc.qmd
GIT_AUTHOR_NAME=Alice GIT_AUTHOR_EMAIL=alice@example.com \
GIT_COMMITTER_NAME=Alice GIT_COMMITTER_EMAIL=alice@example.com \
GIT_AUTHOR_DATE="@1700000000 +0000" GIT_COMMITTER_DATE="@1700000000 +0000" \
  git -c commit.gpgsign=false commit -q -m "alice: initial"
printf 'line1\n世界\nline3\nline4\n' > doc.qmd
git add doc.qmd
GIT_AUTHOR_NAME=Bob GIT_AUTHOR_EMAIL=bob@example.com \
GIT_COMMITTER_NAME=Bob GIT_COMMITTER_EMAIL=bob@example.com \
GIT_AUTHOR_DATE="@1700100000 +0000" GIT_COMMITTER_DATE="@1700100000 +0000" \
  git -c commit.gpgsign=false commit -q -m "bob: append"
git blame --porcelain doc.qmd  # → multi-commit.porcelain
```

Note: the fixtures included here are hand-written approximations of
what real `git blame --porcelain` produces — the hashes are placeholder
40-char hex strings (`aaa...`, `bbb...`) rather than real commit IDs.
The parser shouldn't care about commit-ID content as long as the shape
(40 hex chars, space-separated line numbers) is correct.
