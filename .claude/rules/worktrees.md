# Worktrees

## Directory Convention

All worktrees live in `.worktrees/` at the project root. This directory is git-ignored.

## Branch naming

- **GH issue triage worktree** → branch `issue-<N>` at `.worktrees/issue-<N>/`. Local branch stays bare; only the remote uses a prefix (see § Pushing for PR).
- **Beads issue investigation worktree** → branch `beads/<id>-<slug>` at `.worktrees/<id>-<slug>/`, where `<slug>` is a short kebab-case form of the issue title (3–5 words, lowercase).

The directory mirrors the leaf of the branch name. The conventions are stable so colleagues and tooling can recognize a worktree's origin from the path alone.

## Fresh worktree bootstrap

Use `cargo xtask create-worktree <bd-id>` (or `--issue N` / `--upgrade`) for new worktrees —
it handles `git worktree add`, `.beads/redirect`, and the CLAUDE.local.md context stub in
one shot. After it finishes, run `npm install` from the new worktree if hub-client is in scope:

```bash
cargo xtask create-worktree bd-XXXX
cd .worktrees/<id>-<slug>
npm install                              # only if hub-client work is in scope
cargo xtask verify --skip-hub-build      # confirm green at branch HEAD
```

If the xtask is not yet built (fresh clone, or a branch where `cargo build -p xtask` has
not run), see § Manual bootstrap below.

`cargo xtask dev-setup` exists for Rust dev tools (cargo-nextest, wasm-bindgen-cli) but
does not currently run `npm install`. bd-7giz tracks extending it.

## Beads Redirect

This project uses `br` for issue tracking. After creating any worktree, add a redirect file so `br` uses the main project's database. The `.beads/` directory already exists in the worktree (tracked by git) — just add the `redirect` file alongside the existing files. Do NOT delete or overwrite tracked `.beads/` content.

```bash
# .beads/ already exists from git — just add the redirect
# 3 levels up from .worktrees/<name>/.beads/ to reach project root
echo "../../../.beads" > .worktrees/<name>/.beads/redirect
```

The `redirect` file is already in `.beads/.gitignore`, so it won't show as a git change. Verify with `br where` from inside the worktree.

## CLAUDE.local.md

`cargo xtask create-worktree` prepends a worktree context section to `CLAUDE.local.md`.
Claude Code loads it automatically — no need to run `br show` to orient at session start.

The section contains: worktree declaration, main repo path (`../..`), beads ID,
GitHub URL, and a placeholder for the plan file path (fill in manually after creating
the plan).

Status lives in beads, not in this file. Run `br show <id>` for current status + notes.

The section is delimited by `<!-- BEGIN/END WORKTREE CONTEXT -->` markers so it can be
refreshed in place (e.g. when a worktree is recreated, or by hand-editing the file).
The `update_claude_local_md` rewrite is idempotent at the file level: re-running it on
a file that already has a managed section replaces that section without duplicating it
and preserves any user content below.

`cargo xtask create-worktree` itself is **not** idempotent end-to-end — `git worktree add`
fails fast if the directory already exists. To refresh a worktree's CLAUDE.local.md,
either edit it by hand (the markers make this safe) or remove the worktree and recreate.

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

## Manual bootstrap

If `cargo xtask create-worktree` is unavailable (fresh clone before first build, or
the xtask binary is broken on the current branch), fall back to manual setup:

```bash
git worktree add -b beads/<id>-<slug> .worktrees/<id>-<slug> main
echo "../../../.beads" > .worktrees/<id>-<slug>/.beads/redirect
# Optional but recommended: write a CLAUDE.local.md context stub manually
# using the template from `cargo xtask create-worktree --help` output.
```

Verify with `br where` from inside the worktree.
