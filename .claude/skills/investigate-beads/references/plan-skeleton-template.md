# Plan-skeleton template

Copy the body below into `claude-notes/plans/YYYY-MM-DD-<slug>.md` and fill it in. The plan **is a skeleton, not a finished plan** — phases are draft headings; the design questions section is where the real thinking still has to happen with the user.

```markdown
# <Issue title> (bd-XXXX)

**Date:** YYYY-MM-DD
**Beads:** bd-XXXX
**Worktree:** `.worktrees/<id>-<slug>` (branch `beads/<id>-<slug>`, based on `main` @ `<short-sha>`)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

One of:
- **Ready to design.** Context is clear; this plan sketches phases and lists design questions. Once those are settled, ready to implement.
- **Needs more info.** Specific questions (below) must be answered before this can be scoped.
- **Not ready / blocked.** Prerequisites unmet (list them), OR `discovered-from` context suggests this is overtaken / should be closed. Recommendation: <close | defer | wait on bd-YYYY>.

State the verdict in one sentence. The rest of the plan justifies it.

## Issue context

Quote or paraphrase the issue description. Note status, priority, type, age.

## Dependency graph

What the `dep tree` looks like, and what each edge tells us:

- **discovered-from**: <parent> — the original session was working on X when this surfaced because <Y>.
- **blocks**: <dependent issues, open or closed> — implies <urgency / no-longer-relevant / etc.>
- **related**: <neighbors> — useful as <model for how this kind of work usually looks here>.

If the graph is empty, say so explicitly — it changes the calculus (no incoming pressure, no clear context).

## What the code looks like today

Spot-check report: do the file paths in the description still exist? Has the area been refactored since the issue was filed? Is the symptom the issue describes still reproducible at HEAD?

If reproducible at HEAD, capture the smallest repro under `claude-notes/plans/<slug>-investigation/`.

If NOT reproducible (the issue may have been incidentally fixed), say so and recommend close.

## Proposed phases (draft)

Skeleton only — actual phase contents wait on the design discussion.

- Phase 0 — Test plan (TDD: failing tests written first).
- Phase 1 — <core change>
- Phase 2 — <integration>
- ...
- Phase N — Docs

## Open design questions for the user

Concrete, answerable questions that will let us turn the skeleton into a real plan. Examples:

1. **Scope.** Is this change limited to <X> or should it also cover <Y>?
2. **API surface.** Should we expose <thing> publicly, or keep it internal?
3. **Behavior under <edge case>.** What's the expected behavior when ...?

If the verdict is "not ready / blocked," replace this section with a "What's missing" list — what would have to land first.

## Risks / tradeoffs (draft)

If anything is already obvious from the investigation (e.g. "this touches a stage that has no tests", "this conflicts with bd-YYYY's direction"), note it. If you're not sure yet, say so.
```
