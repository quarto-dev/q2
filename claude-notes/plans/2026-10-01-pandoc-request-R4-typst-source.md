# Plan: Typst source request (pandoc-request R4)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-request-epic.md`](2026-10-01-pandoc-request-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (D8.6, D9, D5 typst case)
**Depends on:** R2; R0's rebase (PR #766). **Unblocks:** R5 (serial), R6's typst case, R7, R8, the typst menu entry (host H5) and the PDF work (host H8).
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** R (workspace-6). **Start gate:** R2 is fully ticked. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** R5 (its Start gate decides whether it can begin).

## Overview

Typst source (`.typ` only, no compile) comes first among the increments after the docx slice because it proves the architecture: template partials, brand, params and the virtual filesystem all pass through the request. The PDF work (host H7-H9) builds on this request.

## Decisions

- The downloaded `.typ` is one file: images and brand logos it references are not bundled, and `typst-available-fonts` is `None` (D8.6).
- A typst document with a remote image fails until R6 lands the prefetch (D5): this phase documents that limitation.
- The `pub(crate)` typst helpers were moved out of the gated module in R2.

## Checklist

### Tests first
- [ ] A typst fixture's request argv/env/files equal the recorded native run (R0 wrapper), with the `.typ` limitations noted in the test.
- [ ] A typst document with a raw HTML table: the wasm request's params carry no `quarto-cli-path` (PR #766's `inline_css` is a no-op on wasm, so the table is not CSS-inlined until R8; design D9).

### Tasks
- [ ] The typst writer request: template dir/partials (`TYPST_TEMPLATE_DIR`, user template and partials as `files`), params, section numbering and heading shift, brand handling, and the `--defaults` toc yaml (`pandoc_write.rs:~893`).
- [ ] Typst-only `std::fs`/`fetch_url`/`Command` sites (evidence §4) become VFS-backed or skipped, including `typst_highlight.rs`'s `std::fs::read_to_string(doc_dir.join(style))` for a `.theme` highlight style (given a runtime parameter, since the `.theme` file is read into argv text, not mounted) and the `typst fonts`/package-extraction sites.
- [ ] **Brand fonts:** `source: file` fonts come in through `resource_refs`; Google/URL brand fonts and the font cache (`font_cache_dir`, `stage_brand_fonts` in `typst_google_fonts`, which write to disk) are skipped on wasm and the limitation documented (D8.6).
- [ ] Document the remote-image limitation in the response envelope's diagnostics so host H5 can report it, and the CSS-inlining limitation above (fixed by R8).
- [ ] **PDF-ready request (consumed by host H8):** the `pdf` format-table entry (flagged hidden from the menu until host H8), `post: compile_typst` with `output_path = *.typ`, the vendored `TYPST_PACKAGES_DIR` and Font Awesome fonts in a separate typst-assets export with its own version (not part of the share tree docx workers mount), `typst_available_fonts` honoured when given, and the document date pinned from `source_date_epoch` for the compile (typst.ts has no date option, so the clock otherwise sets the PDF's `/CreationDate` and `/ID`). The download extension comes from the format table (the typst `Format`'s `output_extension` is `pdf`, and `FormatIdentifier::Pdf` is deliberately not a pandoc-hybrid format, `format.rs:244`).
- [ ] **Open:** pandoc never reads typst images or brand fonts; they matter only to H8's compile. Decide whether to collect them into `resource_refs` only when `post: compile_typst`, since for a `.typ`-only download they would count against the size limits for nothing.

## Verification

Crate-scoped clippy/nextest, the wasm compile check, the hub-client wasm test; typst fixtures through the host's parity net once H3 exists.

## Exit

A typst request equals the native recording structurally and replays through `_start` in Node; the H5 menu entry and the PDF work then have their input.

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
