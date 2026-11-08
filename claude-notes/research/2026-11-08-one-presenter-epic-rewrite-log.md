---
title: 'One-presenter epic: rewrite log (2026-11-08)'
date: 2026-11-08
description: 'What changed when the one-presenter-path epic was rewritten on 2026-11-08 from a proposed plan list into seventeen plans for autonomous agents, and why, so the epic can read as the current truth. Old means the epic at 4099022dc.'
---

The epic was rewritten, not appended to. "Old" means the epic at `4099022dc` (itself the first 2026-11-08 rewrite).

| Old section | Now | Why |
|---|---|---|
| Goal (three scope items) | kept; scope item 3 names `q2 record export-positron` as the third consumer; marimo noted as a documented exception | review found `convert` has no record option yet; decision D12 |
| Decisions 1 to 6 | D1 to D13, finer and with the later decisions | the discussion settled the loop merge, spans, forwarding, handshake, parity gate, docs, scope rule and strands |
| What the spikes found | shortened and merged with the review results, with ran, read and relied marks | evidence moved to the findings note |
| Open questions 1 to 5 | removed; three remain as decisions Gordon owns (R5, retained fallback and opt-out, Julia scope) | the others were decided |
| Plans OPR-0 to OPR-8 (proposal) | OPR-0a to 0d, 1a, 1b, 2, 3, 4a to 4d, 5, 6, 7a to 7d, 8, each its own file | each plan is a unit of merge with one exit; OPR-3 returned as the Julia wire plan; OPR-1 and OPR-4 and OPR-7 split; OPR-0 split by deliverable |
| (dependencies) | each plan depends only on complete or cancelled plans; serial where plans share files | agent B: OPR-1, 2, 3 and 4 share files; OPR-6 tests would exercise behavior OPR-1b changes |
| (docs inside plans) | all docs in OPR-8 | the epic merges as one pull request (Gordon) |
| (new) | lanes A to D with workspaces 3 to 6, `workspace-1` as the integration checkout, claim and landing rules in the protocol | no orchestrator; agents run plans directly |
| (new) | "What Gordon does between plans" and "Decision points" tables | conditional plans and spike results can change the list |

Statuses at the start: `ready` for OPR-0a, 0b, 0c, 0d, 1a, 1b, 2, 3, 4a, 6, 7a, 7b, 7c, 8 (a `ready` plan still waits for its
dependencies); `conditional` for 4b, 4c and 4d; `blocked-on-gordon` for 5 and 7d.

What I did not change: Gordon\'s earlier decisions (forward-slash default, kernel equivalence on the import side only,
the temporary image rule, `q2 convert` replacing `q2 record`, command 1 live-only). The RIU follow-up plan still carries
Items 1 to 6 and 8; its Item 9 has a pointer to OPR-5. Nothing was pushed. The throwaway branch
`spike/one-presenter-s1-s3` exists locally only and must not be merged or pushed.
