---
title: 'Epic: RIU follow-up (RFU), did we build what the records-in-use epic intended'
date: 2026-11-08
description: 'Closes what the RIU-6 demo exposed. R through knitr is characterized (Positron accepts a raw q2 knitr record; the export gate is the only blocker), and what remains is a command rename (q2 convert), the export and import-gate change, test and verification gaps, a Windows run, a docs plan, and a Positron-fork plan. Eight plans run in the same four lanes and on the same integration branch as the one-presenter epic, with dependencies arranged so both epics run concurrently.'
status: 'plans written 2026-11-08; replaces the single follow-up plan; Gordon decisions closed 2026-11-09 (Phase A verdict, fork commits, command 2); runs concurrently with the one-presenter epic on feature/one-presenter'
---

**Replaces:** the single plan `2026-11-07-riu-follow-up-did-we-build-it.md` (deleted 2026-11-08; its content is
below and in the plans). **Follows:** [the records-in-use epic](2026-11-02-records-in-use-epic.md) (done except the
Windows run). **Runs beside:** [the one-presenter epic](2026-11-07-one-presenter-path-epic.md) (OPR), which holds the
**one execution DAG** for the integration branch: lanes, workspaces and the combined dependency table.
**Evidence:** [RIU-6 commands findings](../research/2026-11-07-riu-6-commands-findings.md),
[RIU-6 plan](2026-11-02-riu-06-positron-command-demo.md),
[review note](../research/2026-11-07-riu-follow-up-review.md),
[R through knitr, Phase A](../research/2026-11-07-r-through-knitr-phase-a.md).
**How to run a plan:** [agent protocol](2026-11-08-opr-agent-protocol.md) (it serves both epics).

## What is established

- The records-in-use epic intended that a q2 execution record is the one place outputs live, and that Positron can
  read it, write it, and preview from it. RIU-6 showed that for Python and Julia.
- **R through knitr (Phase A, 2026-11-07, Playwright):** Positron accepts a raw knitr record unfiltered (8 of 8
  cells matched by hash) and shows every output; its own hashes for R and Julia cells equal q2's. The only thing
  stopping `export-positron` on a knitr document is the owner rule (`EXPORT_OWNERS`,
  `crates/quarto-core/src/record/positron_export.rs:101`, `SkipReason::EngineNotExportable`). Positron shows what the
  record holds, so `results: hide` output, `asis` markdown and raw HTML appear as text; that is Positron\'s behavior,
  not a q2 workaround. Nothing was watched by a person.
- The knitr record has three kinds of cell, all structured and owned by `knitr`; none is a fallback cell. The content
  hash is computed in core for every engine from the block text including `#|` lines
  (`crates/quarto-core/src/record/ids.rs:3-26`); it equals Positron\'s for R and Julia cells; Positron\'s parser reads
  `#|` lines of a Julia cell as for Python and R (2 of 2 matched).
- The import direction already handles knitr-owned units (KNR-5). RIU-3b (stamping) landed 2026-11-07.

## Gordon\'s decisions (recorded; not to be reopened by a plan)

- **Principle:** q2 does nothing special for Positron; where q2 produces a compatible record Positron shows it.
  Positron-side feature design is out of scope.
- The export shrinks to what a valid cache file needs. Kernel equivalence is declared on the **import** side only.
- The image-representation rule stays, commented as temporary, until the fork\'s view zone
  (`quartoOutputViewZone.ts`) chooses one representation.
- The `--source-uri` default is the document path written with forward slashes (`quarto_util::to_forward_slashes`).
- `q2 convert --to positron-ipynb` replaces `q2 record` (RFU-1; the ipynb and qmd side is strand `bd-ju7u8a73`).
- `--records` is deleted with no alias; the record source list becomes the `execute: record-sources` metadata key, set with `-M` on the command line (decision R5, folded into OPR-5).
- This is a demo. "Ship" means the demo in the fork; each loose end is fixed for the demo or written down as a known
  issue in the demo\'s run guide.
- Command 1 is live-only (never calls q2); persisting span outputs in records is an eventual goal, not demo work.
- The Viewer\'s big X leaves q2 running: document it (Stop works); do not fix.
- Command 2 replaces a matched cell\'s outputs with no prompt; the preview banner text is left as is; stamped cells
  carrying no `data-loc` is fine; RIU-3b was wanted and is done.

## Decisions closed 2026-11-09 (Gordon; not to be reopened by a plan)

- **The Phase A verdict.** RFU-2 proceeds as proposed: the export shrinks to a valid cache file and knitr cells export. The
  export drops nothing the document says to hide: a `results: hide` output, `asis` markdown and raw HTML go in as the
  record holds them, and Positron shows them as text. The image rule stays (temporary). Reason: the principle above.
  Recorded in RFU-2.
- **Fork commits allowed** on `q2/rfu-7-demo-followups`: yes (local, never pushed). Recorded in RFU-7.
- **Command 2: progress and cancel, and the dirty-document wording: document, do not fix.** Both are known issues in the
  demo run guide (no cancel; the 300 s timeout is the stop; the dirty-document note and the matched/skipped report stay).
  Recorded in RFU-7.

## Plans

All plans use the shared integration branch `feature/one-presenter`, the lanes and workspaces of the
[one-presenter epic](2026-11-07-one-presenter-path-epic.md#plans-lanes-and-dependencies), and the
[agent protocol](2026-11-08-opr-agent-protocol.md). The combined dependency table there is the single execution DAG.

| Plan | What | Depends on | Lane | Status at start |
|---|---|---|---|---|
| [RFU-1](2026-11-08-rfu-1-convert-command.md) | `q2 convert --to positron-ipynb` replaces `q2 record export-positron` | nothing | C | ready |
| [RFU-2](2026-11-08-rfu-2-knitr-export-and-import-gate.md) | knitr cells export; import gate by declared kernel equivalence | RFU-1, OPR-3 | B | ready |
| [RFU-3](2026-11-08-rfu-3-positron-splice-tests.md) | EXR-16 sweep test and the labeled-cell `fig-cap` splice test | OPR-1b | A | ready |
| [RFU-4](2026-11-08-rfu-4-verification-and-small-fixes.md) | Hub verification, decision 12 re-check, highlight check, Julia server messages | OPR-5 | B (was C) | ready |
| [RFU-5](2026-11-08-rfu-5-julia-e2e-flake.md) | The Julia e2e flake: find the EPIPE closer (a note) | nothing | B | ready |
| [RFU-6](2026-11-08-rfu-6-windows-run.md) | The Windows run | OPR-6, RFU-1, RFU-2 | (Gordon) | blocked-on-gordon |
| [RFU-7](2026-11-08-rfu-7-positron-fork.md) | Positron fork: extension update, demo guide, e2e, cache location, command 2 | RFU-1, RFU-2, OPR-5 | A (was C) | ready |
| [RFU-8](2026-11-08-rfu-8-docs.md) | Docs for `q2 convert` and the export, written with `/reader-expectations-prose` | RFU-1, RFU-2 (resolved) | B (was D) | ready |

> **Lane change, 2026-11-09.** RFU-7 moved from lane C to lane A, RFU-4 from lane C to lane B, and RFU-8 from lane D to lane B, because lanes A and B went idle. See the [one-presenter epic](2026-11-07-one-presenter-path-epic.md#plans-lanes-and-dependencies).

**Concurrency with OPR.** RFU-1 has no dependency on OPR and lands first in lane C while Gordon decides OPR-0d\'s
question. OPR-5 then waits for RFU-1 (so the shared flag definition absorbs `convert` and the deleted `record`
command is never touched twice). RFU-2 needs OPR-3 (the Julia engine wire) only to keep edits to `ts_protocol.rs` and the
vendored subtree from colliding. RFU-3 waits for OPR-1b because it pins splice output. RFU-4 and RFU-7 wait for OPR-5
because they use the final flag spelling. OPR-8 waits for RFU-8 (one embedded-docs refresh at the very end).

## Where the old plan\'s items went

| Old item | Now |
|---|---|
| Item 1 Phase A (done) | "What is established" above |
| Item 1 Phase A, extend the raw-record experiment | RFU-7 (fork), because it runs in the fork\'s e2e harness |
| Item 1 Phases B and C | RFU-2 |
| Item 1 Phase D | RFU-7 |
| Item 2 | RFU-5 (the q2 side and the experiments) and RFU-7 (the fork changes) |
| Item 3 | RFU-7 (run guide, cache location, e2e additions, command 2) and RFU-4 (the q2-side boxes) |
| Item 4 | RFU-4; the stale comment at `presenter/options.rs:237` moved to OPR-1a; the embedded-docs refresh is in OPR-8 |
| Item 5 | RFU-3 |
| Item 6 | RFU-6 |
| Item 7 | the one-presenter epic |
| Item 8 | RFU-1 (q2), RFU-7 (the fork\'s extension), RFU-8 (docs) |
| Item 9 | OPR-5 (q2), RFU-7 (the fork\'s extension) |

## Eventual goal: span outputs in execution records (not demo work)

Command 1 runs inline expressions out of band and shows the value as an editor decoration, much as Quarto 1 handles
span execution. The eventual goal is that a span\'s output is a first-class record cell: Positron would persist it in
the cache (a Positron-side feature to propose, not to build here) and q2 would export and import it as an inline unit,
the way cells round-trip. Phase A found that Positron skips an inline unit by itself when pushed, so the export\'s
inline-unit skip is already unnecessary for Positron. Start from q2\'s inline units and span hash (`ids.rs:31-36`), the
import and export skip rules for inline units, and records-in-use decision 6 (a Positron record is display-only).

## Out of scope

Everything in the [one-presenter epic](2026-11-07-one-presenter-path-epic.md); the eventual goal above; the Positron
fork\'s own feature design (the cache-location prototype in RFU-7 is the one exception Gordon asked for).
