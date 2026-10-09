---
title: 'Worktree + cargo-target disk reclamation'
date: 2026-05-22
description: 'Adds a `cargo xtask reap-worktrees` command that surveys `.worktrees/` and removes finished worktrees while skipping unmerged or uncommitted ones, reclaiming the build directories that filled the disk.'
status: draft  # drafting — ready for a separate agent to pick up
braid:
  strand: bd-4y8fd
  priority: P2
  labels: [disk-space, tooling, worktrees]
---

## Motivation

On 2026-05-22 the working machine ran out of disk space mid-session
and blocked all tool execution (the Claude Code harness couldn't
write tool output to `/tmp`). Root cause: five idle worktrees under
`.worktrees/` consuming a cumulative \~220 GB.

Per-worktree breakdown observed before cleanup:

| Worktree            | Size  | Status                |
| ------------------- | ----- | --------------------- |
| `issue-196`         | 44 GB | clean, work landed    |
| `issue-201`         | 45 GB | clean, work landed    |
| `issue-206`         | 47 GB | clean, work landed    |
| `issue-222`         | 36 GB | clean, work landed    |
| `pr-159-merge`      | 49 GB | merge-conflict scratch space |

All five were investigation worktrees from prior sessions whose
work had already merged to main. Nothing was actively in flight;
the disk was being held hostage by completed work that nobody
swept up.

The dominant component in each worktree is `target/` — Rust build
artifacts. CLAUDE.md cites \~60 GB per repo for a warm Q2 build;
five warm worktrees is the disk in this incident.

## Explicit non-goal: shared `CARGO_TARGET_DIR`

A shared cargo target directory across worktrees would collapse
five `target/` trees into one. **User has declined this for now.**
Cross-branch builds churn the incremental fingerprinting cache,
and the correctness risk (a worktree on branch A rebuilding state
that branch B was relying on) isn't worth the disk savings.

If this is revisited in the future, the path is:
- Set `CARGO_TARGET_DIR=$(git rev-parse --git-common-dir)/../cargo-target`
  (or similar) globally for the repo.
- Validate on one worktree first: switch back and forth between
  branches, observe whether incremental builds stay correct.
- Document the trade-off explicitly in CLAUDE.md.

Out of scope for this plan.

## Scope

Two pieces of work, both within `crates/xtask/` and `.claude/skills/`:

### 1. `cargo xtask reap-worktrees`

A subcommand that surveys `.worktrees/` and offers to remove
stale entries.

Default behavior (interactive):

```
$ cargo xtask reap-worktrees
Found 5 worktrees:

  issue-196      branch: issue-196        size: 44G  last commit: 7d ago    merged: yes
  issue-201      branch: issue-201        size: 45G  last commit: 6d ago    merged: yes
  pr-159-merge   branch: pr-159-merge     size: 49G  last commit: 21d ago   merged: no  (untracked: deno.lock)
  bd-foo         branch: beads/bd-foo-x   size: 12G  last commit: 1d ago    merged: no  (in progress)

Remove issue-196? [y/N] y
Removed .worktrees/issue-196 (44G freed).
Remove issue-201? [y/N] y
...
Skipping pr-159-merge (untracked files; pass --force to remove anyway).
Skipping bd-foo (last commit <3d ago; pass --force to remove anyway).

Freed 89G across 2 worktrees.
```

Flags:

- `--force` — remove even with untracked files and recent commits.
- `--yes` — non-interactive; remove every eligible worktree.
- `--min-age <days>` — only consider worktrees whose last commit
  is older than this (default: 3).
- `--require-merged` (default true) — only auto-suggest worktrees
  whose branch has merged into main (or another integration
  branch). Toggle with `--no-require-merged`.
- `--dry-run` — print what would happen, change nothing.
- `--format json` — machine-readable output for CI / scripts.

Implementation outline:

- Lives at `crates/xtask/src/reap_worktrees/`.
- Walk `git worktree list --porcelain` to enumerate.
- For each worktree, compute:
  - directory size (du -s or walk),
  - last commit timestamp on the worktree's branch,
  - merged status (`git branch --merged main` membership),
  - untracked-file presence (`git -C <path> status --porcelain -unormal`).
- Use `git worktree remove [--force] <path>` to delete. Never
  bypass git — preserve the branch ref so commits aren't lost.

### 2. Update worktree-spawning skills

Edit the skill files for `/investigate-beads`, `/triage`,
`/upgrade-cargo-deps` so each adds a cleanup-reminder step at the
end of its workflow.

Suggested addition (paraphrase per skill):

> **After landing the work:** once the change has merged into main
> (or the investigation has concluded with no commit needed),
> remove the worktree:
>
> ```
> cd <main checkout>
> git worktree remove .worktrees/<name>
> ```
>
> If you'd rather sweep all eligible worktrees at once, run
> `cargo xtask reap-worktrees`.

The reminder is advisory, not enforcing — some worktrees stay
useful (e.g. an ongoing investigation), and the user is the right
one to decide.

## Sequencing

The two pieces are independent. Either can land first.

Recommended order:

1. **Skills updates** — pure text edits, no code; lands immediately
   and starts shaping behavior even before the xtask exists.
2. **`reap-worktrees`** — gives the user a one-shot way to recover
   from already-accumulated waste.

## Test strategy (TDD)

For `reap-worktrees`:

- Unit tests under `crates/xtask/src/reap_worktrees/tests.rs`,
  using a tempdir with fake worktrees (just directories — no
  need for a real git setup for the dry-run path).
- Real-git integration test that creates a throwaway worktree,
  invokes `reap-worktrees --dry-run --format json`, parses the
  output, asserts the worktree shows up. Then runs with `--yes`
  and asserts removal.
- All tests must compile and pass on macOS, Linux, Windows
  (cf. `.claude/rules/cross-platform.md` — git-worktree CLI
  behaves consistently, but du/sizing helpers need cfg gates).

For skills updates: no automated test. End-to-end verification
is "render the skill markdown and read it."

## Out of scope

- The `pr-159-merge` style "merge scratch space" worktree. These
  don't follow the `issue-N` / `beads/bd-X-Y` convention this work
  targets. They can still be removed manually; the xtask just
  won't auto-suggest.
- Shared `CARGO_TARGET_DIR` (see above).
- Cleaning other large directories (`node_modules/` in each
  worktree, `.venv/` if any, etc.). Each adds correctness risk
  similar to shared `target/`; defer.
- A session-end hook that runs `reap-worktrees` automatically.
  Hooks for destructive operations need more thought; the xtask
  itself is the v1 deliverable.

## Open questions

1. **Default `--min-age`.** Plan says 3 days. Could be tighter (1
   day) for tighter recovery; could be looser (7 days) for safety.
   Pick after a week of use.
2. **Default integration branch for merged-check.** Plan assumes
   `main`. The repo also uses long-lived `feature/<name>`
   integration branches (per `.claude/rules/worktrees.md`). Should
   worktrees whose branches merged into `feature/<name>` also
   count as eligible? Probably yes; needs spec.
3. **Logging.** Should removal write to a journal (e.g.
   `.worktrees/.reaped.log`) so we can audit what was deleted?
   Probably overkill for v1, but worth noting.
