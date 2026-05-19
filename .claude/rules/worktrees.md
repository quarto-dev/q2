# Worktrees

## Two patterns, two commands

Worktrees and sub-task branches are separate concerns. Pick based on
the *shape* of the work, not on the beads-issue boundary.

- **`cargo xtask create-worktree`** — spin up a *new* worktree at
  `.worktrees/<id>-<slug>/`. Right for **parallel** or
  **investigation** work that benefits from isolation:
  - `/investigate-beads` digs into a single issue without touching
    the main checkout.
  - `/triage` records context on a GH issue without committing on
    top of in-flight work.
  - `/upgrade-cargo-deps` runs full verification on a throwaway
    branch.
  - A reviewer wants to check out a colleague's branch *while* you
    keep working.

  These pay the fresh-worktree cost on purpose: `npm install` from
  cold, `cargo build` from cold, no pollution of any other working
  state. That's a feature, not a tax.

- **`cargo xtask switch-task <bd-id>`** — *reuse* the current
  worktree's checkout, swap the branch in place. Right for
  **sequential** implementation work inside an epic, where each
  sub-task branches off the same integration line and benefits from
  keeping `node_modules/` + `target/` warm.

  Usage at sub-task hand-off:

  ```bash
  # finish the previous sub-task (commit, close beads issue, etc.)
  cargo xtask switch-task bd-yxqt --from feature/q2-preview-command
  ```

  That switches the current worktree to `feature/q2-preview-command`,
  fast-forward-pulls (so any sibling sub-tasks that merged in the
  meantime show up), creates a fresh `beads/bd-yxqt-<slug>` topic
  branch off the new tip, marks the beads issue `in_progress`, and
  rewrites the `CLAUDE.local.md` context block. Omit `--from` to
  branch off the current HEAD.

The two commands are siblings — `create-worktree` does *more* (it
adds a worktree); `switch-task` does *less* (it stays in place). Use
whichever matches the work.

## Integration-line convention

Epic work should accumulate on a long-lived **integration branch**
(commonly `feature/<short-name>` — e.g. `feature/q2-preview-command`).
Each sub-task lives on its own topic branch. When a sub-task closes:

```bash
git switch feature/q2-preview-command
git merge --no-ff beads/<id>-<slug>
git push origin feature/q2-preview-command
```

The `--no-ff` preserves the sub-task as a single merge commit, so
the integration branch's history reads as one entry per sub-task.
This is what `switch-task --from <branch>` expects to find when it
fast-forward-pulls — a clean integration line with all ready work
already merged in.

## Directory Convention

All worktrees live in `.worktrees/` at the project root. This directory is git-ignored.

## Branch naming

- **GH issue triage worktree** → branch `issue-<N>` at `.worktrees/issue-<N>/`. Local branch stays bare; only the remote uses a prefix (see § Pushing for PR).
- **Beads issue investigation worktree** → branch `beads/<id>-<slug>` at `.worktrees/<id>-<slug>/`, where `<slug>` is a short kebab-case form of the issue title (3–5 words, lowercase).
- **In-place sub-task branch (via `switch-task`)** → branch `beads/<id>-<slug>` *without* a corresponding `.worktrees/` directory; the branch lives wherever the caller's worktree is checked out.

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

`--base` defaults to `main` when omitted. **If the beads issue has an
open parent epic, the command prints a warning** nudging you toward
the epic's integration branch (e.g. `feature/<name>`). Pass
`--base <branch>` to branch off the integration line, or `--base main`
explicitly to silence the warning when `main` really is what you want.
For sequential sub-task work *inside* an existing worktree, reach for
`cargo xtask switch-task` (see the two-patterns section above) — it
fast-forwards the integration branch and branches off its current tip
automatically.

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

The section contains: worktree declaration, main repo path (`../..`), beads ID
(or `**GitHub issue:** #N` in `--issue` mode), GitHub URL when available, an
italic-prose placeholder for the plan file path, and a `**Skill:**` line
naming the slash-command that continues the work (`/investigate-beads`,
`/triage`, or `/upgrade-cargo-deps`). Placeholders are self-documenting —
they say exactly what to replace them with.

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
```

Verify with `br where` from inside the worktree. CLAUDE.local.md is not part
of the manual bootstrap — once the xtask binary is built, re-running
`cargo xtask create-worktree` is not safe on the existing worktree (see
above), but the template lives in `crates/xtask/src/create_worktree.rs`
(`build_section`) for hand-copying if needed.
