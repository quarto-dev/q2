---
name: investigate-beads
description: Investigate a braid strand (bd-XXXX issue) in the current checkout, gather context from its dependency graph, and produce a plan-skeleton + triage verdict (ready / needs-info / blocked). Use when the user provides a strand ID (bd-XXXX) and asks to investigate, scope, or understand the work needed.
---

# Investigate-Beads Skill

> Naming note: this skill keeps the `investigate-beads` name (and `/investigate-beads`
> invocation) for stability, but the project now tracks work in **braid** — a strand
> is one issue in the braid skein. Commands below use `braid`.

Takes a braid strand from "user pointed at it" to "a plan skeleton and a triage verdict committed to the current branch." It works in whatever checkout you invoke it in and does **not** create or switch worktrees or branches — you choose where the work lands. It is the strand counterpart to `triage` (which handles GitHub issues, where no branch exists yet and one must be created).

Does **not** implement the fix or finalize the design. Produces enough context to start a focused design session — or to recommend that the issue isn't ready yet.

## When to use

User says any of:
- "investigate bd-XXXX" / "let's look at bd-XXXX"
- "what would it take to work on bd-XXXX"
- pastes a strand ID and asks for context / scoping

**Do not** use for:
- Strands you've already scoped (just edit on `main` or an existing branch)
- GitHub-originated issues — use `triage` instead, which handles the GH side and files a braid strand if needed
- Strands you're about to implement immediately in the current session — `braid update <id> --status in_progress` and start working; this skill's overhead only earns its keep when the strand needs context-gathering before scoping

## Outcome: two durable artifacts

1. A plan skeleton at `claude-notes/plans/YYYY-MM-DD-<slug>.md`, committed to the current branch (along with any investigative artifacts).
2. A triage verdict in the plan, plus design questions for the user — one of:
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
braid show <id> --json
```

(braid's `show --json` is a single object, not an array — read the description, status, type, priority, dates. Note who created it and when — old strands often have stale assumptions worth flagging.)

### 3. Walk the dependency graph

This is the step that earns the skill its keep. A strand's *meaning* is usually richer than its description; the graph carries why-it-was-filed and what-blocks-what.

```bash
braid dep tree <id>        # parent-child descendant tree (epic → subtasks)
braid dep list <id>        # all edges, both directions (blocks / related / discovered-from / ...)
```

For each linked strand, read it the same way. In particular:

- **`discovered-from` chain**: trace it. The originating issue (or session) usually has the context that explains *why* this one was filed — what the parent was trying to do when it surfaced this. Often the most informative single piece of context.
- **`blocks` edges (incoming)**: things that depend on this one. If the dependents are open, they pin the urgency. If they're closed, this issue may already have been addressed differently.
- **`related`**: same area of the codebase; useful for "how is this normally done here."

### 4. Read the referenced plan + code

If the description references a plan file (`claude-notes/plans/...`), read it. If it points at code paths (`crates/foo/src/bar.rs:line`), read those.

Spot-check the area: does the code the strand points at still exist with the same shape? Strands age — a six-month-old strand may have been overtaken by a refactor.

### 5. Write the plan skeleton

This skill does **not** create or switch worktrees or branches. Write and commit in whatever checkout you were invoked in — the user controls where the work lands (a long-running worktree they pointed you at, the current branch, wherever). If you think a different branch or worktree is warranted, *say so and let the user set it up*; do not create one yourself.

Create `claude-notes/plans/YYYY-MM-DD-<slug>.md` (where `<slug>` is a short kebab-case form of the issue title, 3–5 words) using `references/plan-skeleton-template.md`. Put any investigative scratch (small fixtures, exploratory grep output you want to preserve) under `claude-notes/plans/<slug>-investigation/`.

The plan **is a skeleton, not a finished plan.** Phases are draft headings with rough work items; the design questions section is where the real thinking still has to happen *with the user*.

### 6. Plan-skeleton commit

```bash
git add -A
git commit -m "Investigate bd-XXXX: <one-line summary>"
```

Commits to the current branch — whichever checkout you were invoked in. Captures the plan skeleton + any investigative artifacts. Do not leave investigative files uncommitted — they are part of the record.

### 7. Strand: update status, do NOT close

```bash
braid update <id> --status in_progress
```

Even if the verdict is "not ready / blocked," leave the strand in `in_progress` — it has a plan now, which is *progress*. Closing should only happen when the plan recommends close (overtaken / not reproducible) AND the user agrees.

If you discovered any incidental work, file each as its own strand and link with `--deps related:<this-id>` or `--deps discovered-from:<this-id>` on `braid create` (one shot), e.g.
`braid create "..." -t task -p 2 --deps discovered-from:<this-id>`.

braid syncs the skein automatically — **there is nothing to commit** for strand
changes (no more `br sync --flush-only; git add .beads/`). The plan-skeleton
commit in step 6 covers the durable artifacts.

### 8. Hand back to the user

Report:
- the branch the work landed on
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
- **Running the full skill for a 5-minute lookup.** If the user just wants to know what a strand *is*, summarize from `braid show` and stop. The skill's overhead (plan skeleton + commit) earns its keep when the investigation needs to write code (repros, fixtures), not when it's purely descriptive.
- **Creating or switching a worktree/branch on the user's behalf.** This skill works in the checkout it was invoked in. If isolation seems warranted, recommend it and let the user set up the worktree/branch — do not run `cargo xtask create-worktree` / `switch-task` / `git switch` yourself.
