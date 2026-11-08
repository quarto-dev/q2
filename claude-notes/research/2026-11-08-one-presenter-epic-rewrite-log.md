---
title: 'One-presenter epic: rewrite log (2026-11-08)'
date: 2026-11-08
description: 'What changed when the one-presenter-path epic was rewritten on 2026-11-08 as a fresh document with the step-1 decisions, so the epic can read as the current truth.'
---

The epic was rewritten, not appended to. "Old" means the epic at `0fde5025d`.

| Old section | Now | Why |
|---|---|---|
| Goal and "Scope the spikes force us to state" | Goal with three explicit scope items | Question 7 (plain render in scope), question 4 (Julia in scope) and the shared `--execution-records` definition are decided |
| "Decided in the review session" (a short list) | "Decisions", numbered 1 to 6, with the full render-flag rules | The list was a pointer to the decisions note; the epic now carries the rules |
| "What the spikes found" | Kept, with the plain-render finding and the Julia and render-flag evidence added | Evidence belongs beside the decisions it supports |
| "Open design questions" 1 to 7 | Decided ones removed; open ones renumbered (R5, the fallback, option resolution, Jupyter positions, plan order) | Decided questions are decisions now |
| "Still to investigate" | Moved into the OPR-0 boxes | Each unknown became a box with an exit |
| "Plans: none yet" and "Exit criteria: to be written" | A proposed plan list OPR-0 to OPR-8 with order, dependencies, parallelism, exit criteria and who runs each box | The plan list is a proposal for the review sub-agents to test |
| (new) | "Constraints that hold across the plans" and "Out of scope" | Gathered from the intent sources |

Not decided by this rewrite: open question 1 (metadata key versus flag), 2 (the retained fallback and an opt-out),
3 (option resolution), 4 (Jupyter positions) and 5 (plan order). The plan list is a proposal. Follow-up Item 9 still
carries its own boxes (pointer added in `0fde5025d`).
