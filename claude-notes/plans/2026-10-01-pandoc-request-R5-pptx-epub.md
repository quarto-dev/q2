# Plan: pptx and epub requests (pandoc-request R5)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-request-epic.md`](2026-10-01-pandoc-request-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (D3, D1)
**Depends on:** R2; file-serial with R4 (both change `pandoc_write.rs`). **Unblocks:** the pptx and epub menu entries in host H5.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** R (workspace-6). **Start gate:** R4 is fully ticked. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** R6 (its Start gate decides whether it can begin).

## Overview

Extend the docx request to pptx and epub. The only real work is `epub_extra_args`, which extracts `FORMATS_DIR` to disk and passes temp paths to pandoc; in-memory it must become `files` entries.

## Checklist

### Tests first
- [x] pptx and epub fixtures\' requests equal the recorded native runs (R0 wrapper). Epub is expected to need a document `identifier` for reproducibility (R0 exploration).

### Tasks
- [x] `epub_extra_args` (`pandoc_write.rs:152`, `std::fs` + `FORMATS_DIR`) made in-memory: the `--include-in-header` temp paths become `files` entries written under the temp root, beside the share tree; `epub-embed-font` and repeated `css` feed `resource_refs`; `epub-cover-image` and `epub-metadata` likewise (D3).
- [x] pptx: confirm `reference-doc` handling and the lack of a post-step; slide-level and other forwarded args come from `forwarded_args`.

## Verification

Crate-scoped clippy/nextest, the wasm compile check, the hub-client wasm test.

## Exit

All four formats have requests; host H5 lists them from the format table.

## Close-out

- [x] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [x] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [x] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: `pandoc-wasm/r5-pptx-epub`, workspace-6.
- Last commit; tasks ticked: all tasks and Close-out ticked; landed on `feature/pandoc-wasm`. Workspace nextest: 15593 run, 15593 passed (after the sidebar fix below), 202 skipped. R4 reported 15588 passed with 202 skipped; this phase added 4 tests (pptx and epub recording tests, `epub_path_keys_become_resource_refs`, `pptx_forwards_reference_doc_and_slide_level`), so the remaining +1 is most likely the `error_docs_sidebar` test, which was red at R4's end (not verified at R4's commit). Hub-client wasm vitest: 231 passed (229 + 2 new real-pandoc.wasm tests: pptx, epub).
- State and gotchas:
  - `epub_extra_args` was already in-memory (R2): the two `--include-in-header` files are request `files`, and the cover image, metadata, embedded font and `css` flags are `FlagPath` args that `ResourceCollector::add_args` mounts as `resource_refs`. No change was needed there.
  - Real defect found by the pandoc.wasm run: a document's `css:` stayed in the metadata of `pandoc-input.json` as a document-relative value, besides the absolute `--css=` flag. Pandoc reads that metadata value against its cwd (`/` in the browser, so `book.css: openBinaryFile: does not exist`; natively it only worked when the cwd was the document directory) and embedded the stylesheet twice. `css` is now left out of the serialized AST for epub only (`PandocWriteStage::prepare`); the in-memory metadata keeps it. Verified with native pandoc 3.11.
  - The recorded epub argv matches without a document `identifier`; the R0 expectation that epub needs one concerns reproducible output (pandoc generates a UUID otherwise), not the request.
  - The recording holds the whole extracted `formats/` tree; the request carries only the two headers pandoc reads, so the epub test compares those two.
  - Fixed R4's miss: `Q-20-9`/`Q-20-10` pages were not listed in `docs/_quarto.yml`, so `xtask` `error_docs_sidebar` failed.
- Next step: R6 (check its Start gate).
