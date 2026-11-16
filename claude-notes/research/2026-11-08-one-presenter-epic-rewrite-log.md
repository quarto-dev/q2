---
title: 'One-presenter epic: rewrite log (2026-11-08)'
date: 2026-11-08
description: 'What changed when the one-presenter-path epic was rewritten on 2026-11-08 from a proposed plan list into nineteen plans for autonomous agents, and why, so the epic can read as the current truth. Old means the epic at 94906556f.'
---

The epic was rewritten, not appended to. "Old" means the epic at `94906556f` (itself the first 2026-11-08 rewrite).

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
the temporary image rule, `q2 convert` replacing `q2 record`, command 1 live-only). Nothing was pushed. The throwaway
branch `spike/one-presenter-s1-s3` exists locally only and must not be merged or pushed.

## Second change on 2026-11-08: the RIU follow-up folded into the same DAG

Gordon decided to run the RIU follow-up concurrently with this epic on one integration branch, with one execution DAG
and the same four lanes. What changed:

| What | Now | Why |
|---|---|---|
| The single plan `2026-11-07-riu-follow-up-did-we-build-it.md` | deleted; replaced by the [RIU follow-up epic](../plans/2026-11-08-rfu-epic-riu-follow-up.md) and plans RFU-1 to RFU-8 | Gordon: the old plan can be deleted in favor of an epic document |
| Item 9 (the rename) | OPR-5 (already folded in); the fork\'s extension update moved to RFU-7 | one definition, one place |
| Item 8 (`q2 convert`) | RFU-1, first in lane C with no dependency on OPR; OPR-5 now depends on RFU-1 | OPR-5 must not edit the `q2 record` code RFU-1 deletes |
| The unified DAG | in this epic\'s "Plans, lanes and dependencies", with an Epic column; the lane lists interleave OPR and RFU plans | one DAG for both |
| OPR-8 | now also depends on RFU-8 | one embedded-docs refresh at the very end |
| OPR-1a | gained the stale-comment box (`presenter/options.rs:~237`) from the old Item 4 | the file is OPR-1a\'s |
| Protocol | serves both epics: `opr/` and `rfu/` branch prefixes, status checks over both file sets, a paragraph for the fork plan | one set of rules |
| Links | every link to the deleted plan points at the RFU epic | CI renders `claude-notes` strictly |
