# Worktrees

## Directory Convention

All worktrees live in `.worktrees/` at the project root. This directory is git-ignored.

## Branch naming

- **GH issue triage worktree** → branch `issue-<N>` at `.worktrees/issue-<N>/`. Local branch stays bare; only the remote uses a prefix (see § Pushing for PR).
- **Beads issue investigation worktree** → branch `beads/<id>-<slug>` at `.worktrees/<id>-<slug>/`, where `<slug>` is a short kebab-case form of the issue title (3–5 words, lowercase).

The directory mirrors the leaf of the branch name. The conventions are stable so colleagues and tooling can recognize a worktree's origin from the path alone.

## Fresh worktree bootstrap

A fresh worktree has no `node_modules/`. `cargo xtask verify` runs the hub-client TypeScript build, which fails on missing npm deps. Bootstrap with `npm install` from the worktree root before re-running verify:

```bash
cd .worktrees/<name>
npm install
cargo xtask verify --skip-hub-build  # or full verify if hub-client is in scope
```

`cargo xtask dev-setup` exists for Rust dev tools (cargo-nextest, wasm-bindgen-cli) but does not currently run `npm install`. bd-7giz tracks extending it; once that lands, the bootstrap step above becomes a single `cargo xtask dev-setup`.

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

## Pushing for PR

Local branch names stay bare (`issue-<N>`, `beads/<id>-<slug>`). The remote branch name uses a prefix that reflects the work type — `bugfix/`, `feature/`, etc.:

```bash
# GH issue, bug fix
git push -u origin issue-<N>:bugfix/issue-<N>

# Beads issue, feature work
git push -u origin beads/<id>-<slug>:feature/<id>-<slug>
```

This keeps local branches short and consistent while remote refs are self-describing in PR lists.
