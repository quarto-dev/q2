---
title: 'OPR-7d (superseded): switch plain render of Julia and knitr to the recorded route'
date: 2026-11-08
description: 'Tombstone. OPR-7d was split on 2026-11-10 into OPR-9a to 9g by Gordon after an implementability review found it was seven pieces of work, three of which need Gordon in the middle. This file keeps the map from the old decisions and boxes to the new plans.'
status: cancelled
depends-on: []
lane: D
---

**Superseded on 2026-11-10 (Gordon).** Do not start this plan. `cancelled` counts as resolved for the protocol, so
**no plan may depend on `opr-7d` any more**: OPR-8 now depends on `opr-9g`.

**Why.** The [7d implementability review](../research/2026-11-10-opr-7d-implementability-review.md) found that the plan held a normalizer, a
pinned entry kind, zip comparison, five new Julia target classes, a presenter fix, a flip and a preview chain, with three decisions
that need Gordon in the middle, so it could not run in one unattended session. The earlier
[7d plan review](../research/2026-11-10-opr-7d-plan-review.md) produced most of the design that moved to 9d and 9g.

## The pieces

| Plan | What | Depends on | Lane | Status at start |
|---|---|---|---|---|
| [OPR-9a](2026-11-10-opr-9a-non-html-spikes.md) | Six spikes (a note): format matrix, presenter-fix prototype, normalizer prototype, zip extraction, reserved keys, browser replay | nothing | B | ready |
| [OPR-9b](2026-11-10-opr-9b-presenter-follows-target.md) | The presenter picks outputs and names figure directories by target, for every engine (Jupyter changes on non-HTML targets, a separate bugfix) | 7j, 9a | C | ready |
| [OPR-9c](2026-11-10-opr-9c-reserved-unit-id-key.md) | A reserved-key diagnostic for `quarto-unit-id` | 9a | B | ready |
| [OPR-9d](2026-11-10-opr-9d-parity-harness-machinery.md) | Harness: normalizer, pinned entry, zip comparison, a real gate assertion | 9a | B | ready |
| [OPR-9e](2026-11-10-opr-9e-julia-non-html-targets.md) | Julia on non-HTML targets: fixture, parity classes, target allowlist | 9a, 9b, 9d | D | conditional (Gordon answers the 9a decisions) |
| [OPR-9f](2026-11-10-opr-9f-run-the-gate.md) | The gate: four entries proposed, report, Gordon signs | 7a to 7j, 9b to 9e | D | ready |
| [OPR-9g](2026-11-10-opr-9g-flip-plain-render.md) | The flip, binary tests, end to end, preview chain, harness becomes a regression test | 9f | D | blocked-on-gordon |

## Where each old decision and box went

| Old 7d item | Now |
|---|---|
| Start condition, allowlist state | 9f |
| Decision 1 (retained knitr fallback), decision 2 (no opt-out) | 9g |
| Decision 3 (subfigure groups through 7j) | 9f (context), 9g |
| Decision 4 (interleaving accepted), decision 5 (whole-document form, normalizer, pin), the design and traps | 9d (and 9f for the entries) |
| Decision 6 (presenter follows the target) | 9b |
| Goal: the flip, LaTeX family, Jupyter, cached knitr, marimo | 9g |
| Goal: Julia on non-HTML targets | 9e |
| Box: tests first for the normalizer and the pinned entry | 9d |
| Box: entries `proposed`; box: gate before the flip | 9f |
| Box: tests through the binary; flip the default; keep the harness; end to end; preview chain; gates | 9g |
| Box: presenter display priority and figure directory | 9b |
| Box: Julia on non-HTML targets | 9e |
| New from the review: `quarto-unit-id` diagnostic | 9c |
| New from the review: spikes before design | 9a |

The full text of the old plan is in git history (the commit before this one). Marks and evidence carried over are in the new files.

## How other plans\' references to 7d read now

- **OPR-7j\'s box "THE FLIP"** says it sets 7d `ready` when the gate shows no unallowlisted difference except "the four interleaving cases that 7d
  itself allowlists". Read it as: 9g is `blocked-on-gordon` until Gordon signs the entries of 9f; 7j\'s agent does **not** flip anything
  (7j is on its own branch and its file was not edited here). OPR-7g\'s and 7j\'s "7d" mentions mean the series 9a to 9g.
- **The epic** lists the new plans in its table and lanes (edited with this change). Its prose about "OPR-7d" means 9g.
