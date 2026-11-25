---
name: nightly-issue-sweep
description: Unattended overnight sweep of open quarto-dev/q2 GitHub issues. Gates every eligible issue with autonomously-fix-small-issue (posting a marked comment on the ones that fail), opens PRs for up to maxFixes that pass without waiting for CI, and runs one repair attempt on each red or conflicting `autofix` PR. Invoke as /nightly-issue-sweep [maxFixes=N] [dry-run] [issues=1,2,3] [gate-limit=N]. User-invoked only.
disable-model-invocation: true
---

# nightly-issue-sweep

This skill is a thin launcher for the `nightly-autofix` workflow
(`.claude/workflows/nightly-autofix.js`). The per-issue judgement lives in
`autonomously-fix-small-issue` (its gate-only, sweep, and repair modes);
the deterministic parts live in `scripts/`:

| Script | Does |
|---|---|
| `select_candidates.py` | Lists open issues and drops those that are assigned, labelled question/wontfix/duplicate/invalid/cannot-repro, closed by an open PR, cited by a non-closed braid strand, or carrying a current gate marker with no human activity since. |
| `pr_status.py` | Sorts open `autofix` PRs into needs_repair / pending / green / needs_human, counting `<!-- q2-autofix repair n=K -->` markers. |
| `housekeeping.sh` | Resets `.worktrees/autofix` to a fresh detached `origin/main` (stashing orphaned changes, never discarding them), prunes branches of merged autofix PRs, runs `cargo clean` there when disk is low, and says when to abort. It never cleans the main checkout or other worktrees; it only reports their sizes. |

## Markers (the "don't repeat" mechanism)

- **On the issue**, a gate failure that is intrinsic to the issue:
  `<!-- q2-autofix gate v=<N> verdict=fail checks=... -->`. The issue is
  skipped until someone comments on it or edits it, or until the gate
  version is bumped. Environment failures (a broken checkout, network)
  leave no marker and are retried next night.
- **On the PR**, the `autofix` label, plus one repair marker per attempt.
  After 3 attempts, or a fix that would go outside the gated scope, the PR
  gets `autofix:needs-human` and the sweep leaves it alone.

To re-gate everything after changing the gate checklist, bump the gate
version in `autonomously-fix-small-issue/SKILL.md` (§ Recording a gate
failure) and `GATE_VERSION` in the workflow script, together.

## Steps

1. **Parse arguments.** The defaults are `maxFixes=5`, no dry-run, and all
   issues. `issues=` restricts the run to those numbers (useful for
   trials). `gate-limit=` caps how many candidates are gated.
   Disk thresholds can be overridden with `soft-floor-gb=`,
   `hard-floor-gb=`, and `max-target-gb=` (defaults 150 / 50 / 60).
2. **Keep the machine awake.** Start `caffeinate -i -t 43200` with
   `run_in_background`. Tell the user the laptop must be on AC power with
   the lid open (or in clamshell mode with a display), since `caffeinate`
   can't prevent lid-close sleep on battery.
3. **Preflight.** `gh auth status` works, `braid list` prints strands,
   and `.worktrees/autofix` either doesn't exist or `git -C
   .worktrees/autofix status --porcelain` is empty (if it isn't, say so;
   housekeeping will stash the changes). Report anything odd, but don't
   ask unless something is broken.
4. **Run the workflow:**

   ```text
   Workflow({ name: "nightly-autofix", args: {
     root: "<abs path of the main checkout>",
     date: "<YYYY-MM-DD>",
     maxFixes: 5, dryRun: false, issues: [], gateLimit: 0 } })
   ```

   `root` is `$(cd "$(git rev-parse --git-common-dir)/.." && pwd)`. Wait
   for the completion notification. Don't poll.
5. **Write the report** to
   `<root>/.worktrees/autofix-reports/<date>.md` (the directory is
   gitignored; create it with the Write tool). Sections:
   - **PRs opened tonight:** link, issue, one-line behaviour change.
   - **Repairs:** PR, attempt, outcome.
   - **Needs human:** PRs labelled `autofix:needs-human`, with why.
   - **Gate:** counts by verdict; list the passes that weren't fixed
     because of maxFixes, and every `fail-environment` (with its summary,
     since these may point at a broken setup); list intrinsic failures as
     issue + failed checks.
   - **Housekeeping:** free disk at start and end, whether cargo clean
     ran, any orphan stashes, and the other `target/` dirs that could be
     reclaimed by hand.
   - **Notes** from the workflow.
6. **Stop `caffeinate`** and give the user a short summary with the report
   path.

## Tuning maxFixes

Start at 5. Before raising it, look at how previous nights' PRs did:

```bash
gh pr list --repo quarto-dev/q2 --label autofix --state all --limit 100 \
  --json number,state,labels,title \
  --jq 'group_by(.state) | map({state: .[0].state, n: length})'
```

If most PRs merge with little rework and few get `autofix:needs-human`,
raise it. If not, tighten the gate first (and bump the gate version).

## Running it unattended

From an interactive session: type `/nightly-issue-sweep` before leaving.
To schedule it (not yet set up), a launchd job can run it headless, for
example `claude -p "/nightly-issue-sweep" --permission-mode
bypassPermissions` from the main checkout. Interactively-authenticated
MCP servers aren't available there, but `gh` and `braid` are plain CLIs.

## Trial run first

`/nightly-issue-sweep dry-run issues=<3–4 numbers>` gates only those
issues, posts nothing, and returns the comments it would have posted.
