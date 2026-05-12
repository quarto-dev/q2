---
name: investigate-beads
description: Set up an isolated worktree to investigate a beads issue, gather context from its dependency graph, and produce a plan-skeleton + triage verdict (ready / needs-info / blocked). Use when the user says "investigate bd-XXXX", "let's look at bd-XXXX", or pastes a beads issue ID and asks what's needed to work on it.
---

# Investigate-Beads Skill

Takes a beads issue from "user pointed at it" to "isolated worktree on its own branch, with a plan skeleton and a triage verdict committed to it." It is the beads-issue counterpart to `triage` (which handles GitHub issues).

Does **not** implement the fix or finalize the design. Produces enough context to start a focused design session — or to recommend that the issue isn't ready yet.

## When to use

User says any of:
- "investigate bd-XXXX" / "let's look at bd-XXXX"
- "what would it take to work on bd-XXXX"
- pastes a beads ID and asks for context / scoping

**Do not** use for:
- Beads issues you've already scoped (just edit on `main` or an existing branch)
- GitHub-originated issues — use `triage` instead, which handles the GH side and files a beads issue if needed
- Issues you're about to implement immediately in the current session — `br update <id> --status in_progress` and start working; this skill's overhead only earns its keep when the issue needs context-gathering before scoping

## Outcome: three durable artifacts

1. A worktree branch `beads/<id>-<slug>` at `.worktrees/<id>-<slug>/`, with one commit containing the plan skeleton (and any investigative artifacts).
2. A plan skeleton at `claude-notes/plans/YYYY-MM-DD-<slug>.md` on that branch.
3. A triage verdict in the plan, plus design questions for the user — one of:
   - **Ready to design** — context clear, draft phases sketched, design questions ready for alignment.
   - **Needs more info** — specific questions that have to be answered before scoping makes sense.
   - **Not ready / blocked** — prerequisites missing, or `discovered-from` chain suggests the original problem was solved differently and the issue should be closed/deferred.

Investigative artifacts (small repros, exploratory snippets, notes you took while reading the dependency graph) live alongside the plan under `claude-notes/plans/<slug>-investigation/` and are committed with it.

## Steps

### 1. Pre-flight: verify HEAD is green

```bash
cargo xtask verify --skip-hub-build
```

Same rationale as `triage`: catches "the issue is already broken at HEAD" vs. "you introduced it" confusion later, and surfaces environment problems before the user is invested. If `verify` fails for a non-bootstrap reason, stop and tell the user. For fresh-clone bootstrap, see `.claude/rules/worktrees.md` § Fresh worktree bootstrap.

### 2. Read the issue

```bash
br show <id> --json
```

Read the description, status, type, priority, dates. Note who created it and when — old issues often have stale assumptions worth flagging.

### 3. Walk the dependency graph

This is the step that earns the skill its keep. A beads issue's *meaning* is usually richer than its description; the graph carries why-it-was-filed and what-blocks-what.

```bash
br dep tree <id>           # blocks / parent-child / discovered-from edges
```

For each linked issue, read it the same way. In particular:

- **`discovered-from` chain**: trace it. The originating issue (or session) usually has the context that explains *why* this one was filed — what the parent was trying to do when it surfaced this. Often the most informative single piece of context.
- **`blocks` edges (incoming)**: things that depend on this one. If the dependents are open, they pin the urgency. If they're closed, this issue may already have been addressed differently.
- **`related`**: same area of the codebase; useful for "how is this normally done here."

### 4. Read the referenced plan + code

If the description references a plan file (`claude-notes/plans/...`), read it. If it points at code paths (`crates/foo/src/bar.rs:line`), read those.

Spot-check the area: does the code the issue points at still exist with the same shape? Beads issues age — a six-month-old issue may have been overtaken by a refactor.

### 5. Create the worktree (skip if already inside it)

**First, check if you're already in the right worktree.** A `CLAUDE.local.md` whose `**Beads:**` line matches `<id>` means the worktree exists and you're in it — skip to step 6. This skill is often re-invoked from inside an existing worktree to reload context; re-running `cargo xtask create-worktree` from there would fail noisily (`git worktree add` errors on existing directories).

If you're in the main checkout or a different worktree, create it now:

```bash
cargo xtask create-worktree <id>
# Creates the worktree, .beads/redirect, and CLAUDE.local.md context stub.
# Slug is auto-derived from the beads title; pass `--slug X` to override.
# Fallback for fresh clones where the xtask is not yet built:
# see .claude/rules/worktrees.md § Manual bootstrap.
```

Branch + directory naming follows `.claude/rules/worktrees.md` § Branch naming (`beads/<id>-<slug>` where `<slug>` is a short kebab-case form of the issue title, 3–5 words). Beads redirect setup follows § Beads Redirect.

Verify with `br where` from inside the worktree.

### 6. Bootstrap the worktree

See `.claude/rules/worktrees.md` § Fresh worktree bootstrap. Then re-run `cargo xtask verify --skip-hub-build` from inside the worktree to confirm green at branch HEAD.

### 7. Write the plan skeleton

Create `claude-notes/plans/YYYY-MM-DD-<slug>.md` using `references/plan-skeleton-template.md`. Put any investigative scratch (small fixtures, exploratory grep output you want to preserve) under `claude-notes/plans/<slug>-investigation/`.

The plan **is a skeleton, not a finished plan.** Phases are draft headings with rough work items; the design questions section is where the real thinking still has to happen *with the user*.

### 8. Plan-skeleton commit

```bash
cd .worktrees/<id>-<slug>
git add -A
git commit -m "Investigate bd-XXXX: <one-line summary>"
```

Captures the plan skeleton + any investigative artifacts. Do not leave investigative files uncommitted — they are part of the record.

### 9. Beads issue: update status, do NOT close

```bash
br update <id> --status in_progress
```

Even if the verdict is "not ready / blocked," leave the issue in `in_progress` — it has a worktree and a plan now, which is *progress*. Closing should only happen when the plan recommends close (overtaken / not reproducible) AND the user agrees.

If you discovered any incidental work, file each as its own bd issue and link with `--deps related:<this-id>` or `--deps discovered-from:<this-id>`.

### 10. Beads JSONL changes go on `main`

See `.claude/rules/worktrees.md` § Committing beads changes.

### 11. Hand back to the user

Report:
- the worktree path and branch
- the plan-skeleton path
- the verdict in one line
- the design questions verbatim (so the user can respond inline without opening the file)

The user takes it from there: answers the questions to turn the skeleton into a real plan, says "not now," or asks for more investigation.

## Anti-patterns

- **Skipping the dependency graph.** Reading only the issue description loses the "why was this filed" context that `discovered-from` carries. The graph is the highest-leverage step.
- **Writing a finished plan instead of a skeleton.** Real design happens in conversation; if the skeleton already pins the answer, the user has no room to redirect.
- **Closing "not ready" issues unilaterally.** Always make the close recommendation a question for the user, never a unilateral action.
- **Skipping pre-flight verify.** Same trap as `triage`: hides bootstrap problems inside the investigation.
- **Forwarded TODOs in the open-questions section.** Each question should be specific and answerable. "Figure out the design" is not a design question.
- **Putting investigative artifacts in `/tmp`.** They are part of the durable record; commit them under `claude-notes/plans/<slug>-investigation/`.
- **Auto-spawning a worktree for a 5-minute lookup.** If the user just wants to know what an issue *is*, summarize from `br show` and stop. The skill's worktree overhead earns its keep when the investigation needs to write code (repros, fixtures), not when it's purely descriptive.
