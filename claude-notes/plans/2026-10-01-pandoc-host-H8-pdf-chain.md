# Plan: PDF chain (pandoc-host H8)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-host-epic.md`](2026-10-01-pandoc-host-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (PDF section, D8.6)
**Depends on:** H5 (the control and status channel), H7 (typst worker), request R4 (typst-source request); typst documents with remote images also need R6. **Unblocks:** H9.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** H (workspace-7). **Start gate:** H5's demo STOP, H7 and R4 are ticked (R6 too for typst documents with remote images). If it is not met, change nothing, report which gate is open and stop. **Next in lane:** H9 (its Start gate decides whether it can begin).

## Overview

"Download as PDF": the pandoc worker produces the `.typ`, the typst worker compiles it. This phase adds the compile-side request additions and the orchestration, with one progress and diagnostic channel across both stages.

## Decisions

- The PDF request is the typst-writer request from request R4 with `output_path = *.typ` and a `post: compile_typst` step.
- The PDF chain is font-list → request → pandoc → typst compile: the typst compiler is loaded (fonts known) before pandoc runs, and `render_pandoc_request` takes the optional `typst_available_fonts` that request R2's export reserved.

## Checklist

### Tests first
- [ ] A typst fixture goes through pandoc then typst to a valid PDF with the expected page count.
- [ ] A typst fixture with an image and a template import compiles (the compile sees the same tree pandoc did).
- [ ] An abort during either stage terminates the right worker and reports once.

### Tasks
- [ ] **Host side of the compile additions** (request R4's last task supplies the `pdf` format entry, `post: compile_typst`, the packages/fonts in the typst-assets export and `typst_available_fonts`): the typst worker mounts the pandoc request's own tree (share tree, `files`, `resource_refs`, at the same absolute paths) plus the produced `.typ`, with compile root `/`, so images and template imports resolve; hand the vendored `TYPST_PACKAGES_DIR` + Font Awesome fonts to the typst worker; brand `source: file` font bytes from the request's `resource_refs`; `typst-available-fonts` from the font builder's family names for the fonts the typst worker loaded (H7).
- [ ] **Chain:** pandoc worker → typst worker, "Download as PDF" in the H5 menu, progress across both stages; each stage's `diagnostics[]` is tagged `stage: pandoc|typst` and concatenated in stage order on one channel, an error from either blocks the download and warnings from both are shown; one-in-flight and cancel semantics from D8.5.
- [ ] First-use download totals shown in the size hint (pandoc ~16 MB + typst ~11 MB + fonts).

## Verification

Vitest for the chain in Node, Playwright for "Download as PDF" on a typst fixture.

## Exit

PDF download works. H9 adds the viewer and parity checks.

## Close-out

- [ ] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [ ] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [ ] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: _none yet_
- Last commit; tasks ticked: _none_
- State and gotchas: _none_
- Next step: _the first unticked task_
