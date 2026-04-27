# Worktrees

## Directory Convention

All worktrees live in `.worktrees/` at the project root. This directory is git-ignored.

## Beads Redirect

This project uses `br` for issue tracking. After creating any worktree, add a redirect file so `br` uses the main project's database. The `.beads/` directory already exists in the worktree (tracked by git) — just add the `redirect` file alongside the existing files. Do NOT delete or overwrite tracked `.beads/` content.

```bash
# .beads/ already exists from git — just add the redirect
# 3 levels up from .worktrees/<name>/.beads/ to reach project root
echo "../../../.beads" > .worktrees/<name>/.beads/redirect
```

The `redirect` file is already in `.beads/.gitignore`, so it won't show as a git change. Verify with `br where` from inside the worktree.

## Committing beads changes

With a redirect active, all beads data lives physically in the main repo's `.beads/`. JSONL changes from worktree work are only visible in `git status` from the main repo. All beads git commits must happen from the main repo, not from a worktree branch.
