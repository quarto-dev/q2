#!/usr/bin/env bash
# Prepare the shared autofix worktree for the next task and keep disk in check.
#
# Run from anywhere inside the q2 repo. Prints one JSON object on stdout:
#   {"worktree", "free_gb_before", "free_gb", "target_gb", "cleaned",
#    "orphan_stash", "pruned_branches", "abort", "abort_reason", "others"}
#
# Steps:
#   1. Ensure .worktrees/autofix exists (detached at origin/main).
#   2. If its tree is dirty (a crashed task), stash everything under a named
#      stash. Never discard.
#   3. Detach at a freshly fetched origin/main, so every task starts clean.
#   4. Delete local braid/* branches whose autofix PR has merged.
#   5. `cargo clean` in the autofix worktree when free space < --soft-floor-gb
#      or its target/ > --max-target-gb. (A full workspace test build alone is
#      ~60 GB, so a lower cap cleans between every fix.)
#   6. abort=true when free space is still < --hard-floor-gb.
#   With --report-others, also lists target/ sizes of the main checkout and
#   other worktrees (slow: du over 100+ GB). It never cleans those.
#
# Usage: housekeeping.sh [--soft-floor-gb 150] [--hard-floor-gb 50]
#                        [--max-target-gb 120] [--report-others]
set -euo pipefail

SOFT=150 HARD=50 MAXT=120 OTHERS=0
while [ $# -gt 0 ]; do
  case "$1" in
    --soft-floor-gb) SOFT=$2; shift 2 ;;
    --hard-floor-gb) HARD=$2; shift 2 ;;
    --max-target-gb) MAXT=$2; shift 2 ;;
    --report-others) OTHERS=1; shift ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

ROOT=$(cd "$(git rev-parse --git-common-dir)/.." && pwd)
WT="$ROOT/.worktrees/autofix"
log() { echo "housekeeping: $*" >&2; }
free_gb() { df -k "$ROOT" | awk 'NR==2 {printf "%d", $4/1048576}'; }
size_gb() { if [ -d "$1" ]; then du -sk "$1" 2>/dev/null | awk '{printf "%d", $1/1048576}'; else echo 0; fi; }

git -C "$ROOT" fetch --quiet origin main

# 1. ensure the worktree
if [ ! -d "$WT" ]; then
  log "creating $WT"
  git -C "$ROOT" worktree add --detach "$WT" origin/main >&2
fi

# 2. orphaned changes from a crashed task
ORPHAN=""
if [ -n "$(git -C "$WT" status --porcelain)" ]; then
  BR=$(git -C "$WT" branch --show-current || true)
  ORPHAN="autofix-orphan-${BR:-detached}-$(date +%Y%m%dT%H%M%S)"
  git -C "$WT" stash push --include-untracked -m "$ORPHAN" >&2
  log "stashed orphaned changes as '$ORPHAN'"
fi

# 3. clean start
git -C "$WT" switch --quiet --detach origin/main

# 4. prune local branches of merged autofix PRs (remote prefix feature/ or bugfix/)
PRUNED=()
while IFS= read -r head; do
  [ -z "$head" ] && continue
  local_br="braid/${head#*/}"
  if git -C "$ROOT" show-ref --verify --quiet "refs/heads/$local_br"; then
    if git -C "$ROOT" branch -D "$local_br" >/dev/null 2>&1; then PRUNED+=("$local_br"); fi
  fi
done < <(gh pr list --repo quarto-dev/q2 --state merged --label autofix --limit 100 \
           --json headRefName --jq '.[].headRefName')
git -C "$ROOT" worktree prune

# 5. disk
FREE_BEFORE=$(free_gb)
TARGET=$(size_gb "$WT/target")
CLEANED=false
if [ "$FREE_BEFORE" -lt "$SOFT" ] || [ "$TARGET" -gt "$MAXT" ]; then
  log "free ${FREE_BEFORE}G, autofix target ${TARGET}G -> cargo clean"
  (cd "$WT" && cargo clean >&2)
  CLEANED=true
  TARGET=$(size_gb "$WT/target")
fi
FREE=$(free_gb)

# 6. abort?
ABORT=false REASON=""
if [ "$FREE" -lt "$HARD" ]; then
  ABORT=true
  REASON="only ${FREE}G free after cleaning (hard floor ${HARD}G)"
fi

OTHERS_JSON="[]"
if [ "$OTHERS" = 1 ]; then
  OTHERS_JSON=$(
    { echo "$ROOT/target"; ls -d "$ROOT"/.worktrees/*/target 2>/dev/null || true; } |
    grep -v "^$WT/" |
    while IFS= read -r t; do
      [ -d "$t" ] && printf '{"path":"%s","gb":%s}\n' "${t#$ROOT/}" "$(size_gb "$t")"
    done | jq -s 'sort_by(-.gb)')
fi

jq -n --arg wt "$WT" --argjson fb "$FREE_BEFORE" --argjson f "$FREE" \
  --argjson t "$TARGET" --argjson c "$CLEANED" --arg o "$ORPHAN" \
  --argjson a "$ABORT" --arg r "$REASON" --argjson others "$OTHERS_JSON" \
  --args '{worktree: $wt, free_gb_before: $fb, free_gb: $f, target_gb: $t,
           cleaned: $c, orphan_stash: $o, pruned_branches: $ARGS.positional,
           abort: $a, abort_reason: $r, others: $others}' "${PRUNED[@]+"${PRUNED[@]}"}"
