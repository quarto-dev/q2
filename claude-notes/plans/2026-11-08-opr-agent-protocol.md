---
title: 'OPR agent protocol: how to pick, claim, run and land a plan'
date: 2026-11-08
description: 'The rules every agent follows when it runs one plan of the one-presenter epic on its own, with no orchestrator and no sub-agents: how to find a plan whose dependencies are complete, claim it, work in a lane workspace, and land it on the integration branch with a rebase and a fast-forward merge.'
---

**Epics:** [one presenter path](2026-11-07-one-presenter-path-epic.md) (plans `opr-*`) and [RIU follow-up](2026-11-08-rfu-epic-riu-follow-up.md)
(plans `rfu-*`). They share one integration branch, four lanes and **one execution DAG**, which lives in the one-presenter
epic under "Plans, lanes and dependencies". Every plan file points here.
Read this file, the epic's "Decisions" section and your plan, in that order, before touching anything.

## Who you are

You are one agent, running one plan at a time, from start to landing. There is **no orchestrator**. **Do not spawn
sub-agents** (no `Agent` tool calls); do the work yourself. Do not ask Gordon questions unless a box is marked
`[Gordon]` or you are blocked (see "When you cannot finish"). Do not push to any remote, ever. Never use a bare
`git stash`. Scratch files go in your session scratchpad, not `/tmp`.

## The workspaces

| Role | Path | Branch |
|---|---|---|
| Integration checkout (a landing pad: **nobody edits here**) | `/Users/gordon/src/q2/.worktrees/workspace-1` | `feature/one-presenter` |
| Lane A | `/Users/gordon/src/q2/.worktrees/workspace-3` | one `opr/...` or `rfu/...` branch per plan |
| Lane B | `/Users/gordon/src/q2/.worktrees/workspace-4` | one `opr/...` or `rfu/...` branch per plan |
| Lane C | `/Users/gordon/src/q2/.worktrees/workspace-5` | one `opr/...` or `rfu/...` branch per plan |
| Lane D | `/Users/gordon/src/q2/.worktrees/workspace-6` | one `opr/...` or `rfu/...` branch per plan |

Work in your own lane workspace. Run `pwd` before any command that uses a relative path. The one-presenter epic\'s table lists which plans belong to which lane, in order (both epics\' plans are in it).

## Status values (frontmatter `status:` of each plan file)

`ready`, `blocked-on-gordon`, `conditional`, `in-progress`, `complete`, `cancelled`.

A plan is **startable** when all of these hold, read from the **integration branch**, not from your checkout:

1. Its own `status` is `ready`.
2. Every plan in its `depends-on` list is **resolved**: `complete`, or `cancelled` (a conditional plan that was
   dropped counts as resolved; say so in your plan notes if it changes what you do).

Check from your lane workspace (the plan ids map to files `claude-notes/plans/2026-11-08-opr-<id>-*.md` and `...-rfu-<id>-*.md`):

```bash
git grep -h -E '^(status|depends-on):' feature/one-presenter -- 'claude-notes/plans/2026-11-08-opr-*.md' 'claude-notes/plans/2026-11-08-rfu-*.md'
git grep -E '^status:' feature/one-presenter -- 'claude-notes/plans/2026-11-08-opr-*.md' 'claude-notes/plans/2026-11-08-rfu-*.md'
```

A plan is `ready` from the start; its `depends-on` list alone holds it back, so resolving a dependency makes the
plan startable with no manual step. Only `[Gordon]`-labeled work and the statuses `blocked-on-gordon` (work only he can
do) and `conditional` (the plan names a condition, such as odd results, that he must check) need Gordon. He flips those
to `ready` by editing the plan on the integration branch. You never flip them yourself. If you write or change a plan,
do not use `conditional` for plain dependency gating; use `ready` and `depends-on`.

## Picking a plan

Take the first plan in **your lane's list** (in the epic) that is startable. If none is startable, **stop** and
report which plans you are waiting for and what they wait on. Do not poll, do not take a plan from another lane
unless the epic says so, and do not start a plan whose dependencies are not resolved.

## Claiming a plan

Claiming prevents two agents from taking the same plan. Do it first and land it at once.

```bash
git switch -c <opr|rfu>/<id>-<slug> feature/one-presenter    # the branch name is in your plan
# edit the plan's frontmatter: status: in-progress   (nothing else)
git add -A && git commit -m "OPR-<id>: claim"
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only <opr|rfu>/<id>-<slug>
```

If the fast-forward fails because the integration branch moved, run `git rebase feature/one-presenter` and repeat the
merge. If another agent's claim for the same plan is already on the integration branch, stop and report.

## Working rules (from the repository's AGENTS.md and rules; they all apply)

- **Tests first.** Write the test, run it, see it fail for the stated reason, then implement. Use
  `cargo nextest run -p <crate>`, never `cargo test`, and never pipe nextest through `tail`. Gates per crate:
  `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo nextest run -p <crate>`. Do **not** run the
  workspace nextest; the plans name the crates.
- **New integration tests** go in `crates/<crate>/tests/integration/<name>.rs`, registered alphabetically in that
  directory's `main.rs`. Never add a top-level `tests/<name>.rs`.
- **Windows.** Windows is not in CI. For every crate you touch run
  `cargo clippy -p <crate> --all-targets --target x86_64-pc-windows-msvc -- -D warnings` (add the target once with
  `rustup target add x86_64-pc-windows-msvc`). Paths that leave the process (JSON, HTML, URLs) use forward slashes.
- **WASM.** `quarto-core`\'s presenter and the splice stage compile for `wasm32` through
  `wasm-quarto-hub-client` (excluded from the workspace; `cargo tree -i quarto-core` does not list it). A plan that
  touches `presenter/`, `record/`, `stage/stages/record_splice.rs` or `engine/{replay,ipynb_replay,skeleton}.rs`
  must also run `cd hub-client && npm run build:wasm`. Never add `test` to a `wasm32` cfg guard.
- **Async traits** use `#[async_trait(?Send)]`.
- **Snapshots.** When a commit adds, changes or removes `.snap` files or golden files, say how many, what changed
  and anything surprising, in the commit message and in your landing report; list the files.
- **R and Julia tests.** Tests that need R or Julia must skip cleanly when it is absent, as the existing knitr and
  Julia tests do. Check each plan for which tools it needs.
- **Strands.** Do not open a braid strand for work that belongs to a plan. A strand is only for something found
  outside every plan. Never use `br`.
- **Notes.** American spelling. Anything you add under `claude-notes/` must pass
  `target/debug/q2 render --strict <file>` (use the `q2` built in your checkout; the only known error in the
  tree is Q-12-13). Rules for qmd: escape an apostrophe right after a code span (`a.rs`\'s), no indented code blocks,
  no bare angle-bracket text.
- **Disk.** A gate that exits 101 with no test failure may be a full disk. Clear only your own workspace\'s
  `target/debug/incremental`.
- **hub-client.** If a commit changes anything under `hub-client/`, the changelog rule in AGENTS.md applies.
- **Locate code by symbol.** Line numbers in a plan are approximate; locate code by symbol.
- **Do not weaken a test** to make it pass. A failing test you did not expect is a finding: say so.
- **Evidence.** Say which steps you ran, which you only read, and which you did not do. "Tests pass, I did not
  verify the real render path" is an acceptable status.
- **Facts in a plan** carry marks: (read), (ran), or (relied) for a claim taken from a review agent\'s report and
  not checked. Re-check a (relied) line before depending on it.

## Landing a plan

The epic merges to `main` as one pull request, so history on the integration branch stays **linear**: rebase, never
merge commits.

1. Everything is committed in your lane workspace on your `opr/...` or `rfu/...` branch, and the plan\'s own gates are green.
2. Reconcile the plan file: re-read your plan, verify each `- [ ]` and `- [x]` against what actually landed, correct
   any that are wrong, and fill in "Notes from the run" (what you ran, what you found, what surprised you).
3. Set the plan\'s frontmatter `status: complete` in the same branch.
4. Rebase onto the integration branch:

   ```bash
   git rebase feature/one-presenter
   ```

   After any conflict, stop and sweep the whole tree for markers before you continue:
   `git grep -n -E '^(<<<<<<<|>>>>>>>)' -- . ':!claude-notes'` must print nothing, and gate the commit on it.
5. Re-run the plan\'s gates after the rebase (the integration branch moved, so green before the rebase is not enough).
6. Fast-forward the integration branch from the integration checkout:

   ```bash
   git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only <opr|rfu>/<id>-<slug>
   ```

   If it refuses because the integration branch moved again, go back to step 4. Do not use `--no-ff`, `-f` or a
   direct ref update.
7. Verify: `git -C /Users/gordon/src/q2/.worktrees/workspace-1 log --oneline -1` shows your last commit.
8. Report: the plan id, the landed commit, the gates you ran and their results, snapshot changes (counts and
   files), and anything Gordon needs to know. Then end with a handoff (next section).

Leave your branch in place. Do not delete branches or worktrees.

## Ending your session: the handoff

Every session ends with a **handoff**: a self-contained prompt, in your final message, that Gordon can paste into a
fresh session in the same lane. Write it after you have committed and merged (or, if you could not finish, after you
have committed your progress). The next agent has none of your context, so the handoff names everything it needs. Do not
start the next piece of work yourself.

Which handoff depends on how the session went:

1. **Done, no serious problems: hand off the next plan in the lane.** Use this only when your plan is `complete` and
   landed, every gate was green, and nothing surprising came up (no unexpected failure, no finding that changes another
   plan\'s boundary, no question left for Gordon). Name the lane\'s next plan: the first plan in your lane\'s list in
   the epic that is startable, read from the integration branch. If none is startable, say which plans the lane waits
   on and for what, and write no plan handoff.
2. **Not done, or a serious problem: hand off to continue your plan.** Choose one:
   - **Same branch, no merge**, when the work is still the right shape: your commits are on your `opr/...` or `rfu/...`
     branch, nothing of the plan has landed beyond the claim, and the integration branch has not moved in a way that
     invalidates the work. Say the branch name, that the workspace must stay on it, and that it must **not** be
     merged until the plan is finished.
   - **A new branch from the integration branch**, when the old branch is not a good base: a notes commit or partial
     work already landed, the plan or the code it touches changed on the integration branch, or you backed out an
     approach. Name the new branch (the plan\'s name for it, with a suffix such as `-2`), say which commits of the old
     branch to cherry-pick or ignore, and say the claim has already landed so it need not be repeated.
3. **Blocked on Gordon:** say what he must decide or flip, and write no handoff to continue; the plan\'s status
   (`blocked-on-gordon`) is the handoff.

Every handoff contains:

- the lane and workspace path, the plan id and its file, and the branch to use (and whether to create it);
- the instruction to read this protocol, the epic\'s "Decisions" and the plan, in that order, and to run `pwd` first;
- what is done, what is not, and the state of the branch (committed, uncommitted, landed or not, the last commit);
- for a continuation: the reason you stopped, what you tried that did not work, and the next concrete step;
- anything Gordon told you that is not yet in a plan file (there should be nothing; write it into the plan instead).

Keep it short: a handoff points at files and commits rather than repeating them.

## When you cannot finish

If you are blocked (a `[Gordon]` box, an unexpected result that changes the plan\'s boundary, a tool you do not
have), do **not** land a partial plan. Instead:

1. Commit your progress on your `opr/...` or `rfu/...` branch.
2. Add a "Notes from an earlier attempt" section to the plan with what is done, what is not, and why you stopped.
3. Set `status: ready` (or `blocked-on-gordon` if only Gordon can unblock it), commit, rebase, and land **only that
   notes commit** with the fast-forward merge above (cherry-pick it onto a fresh branch from the integration branch
   if your work commits cannot go with it).
4. Report the reason and the branch name in your final message, and end with a handoff to continue (next section).

## A plan in another repository (the Positron fork)

RFU-7 works in the Positron fork, which is not part of `feature/one-presenter`. Its commits go on a fork branch (never
pushed) and only a landing note is merged here; the plan says how. Read the fork\'s own `CLAUDE.md` and `AGENTS.md` first.

## Companions outside the epic (Gordon\'s, not agents\')

Pushes to the Julia engine and runner forks, a push or merge of the Positron fork branch (RFU-7), and any push of the integration branch. A
plan that needs one says so and ends before it.
