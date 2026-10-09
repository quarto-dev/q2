---
title: 'Plan: Typst source request (pandoc-request R4)'
date: 2026-10-01
description: 'Adds a browser-side Pandoc WASM request that produces a single Typst `.typ` source file, carrying templates, partials, brand settings and a pinned document date, as groundwork for PDF output.'
---

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
- The `pub(crate)` typst helpers were moved out of the gated module in R2. (`brand_for_mode` had stayed in the gated `typst_compile.rs`; R4 moved it to `pandoc_write.rs`.)
- The typst template, user template, `template-partials` and the toc-depth defaults file are request `files` for every target (native `execute()` writes them like any other file); a `.theme` highlight file is read through the runtime into argv text, so it is not mounted. A missing `template:` is still silently ignored (the merge's existence-silent policy, unchanged); a missing partial is a stage error.
- `pdf` is a `PANDOC_FORMATS` row with `hidden: true`: `render_pandoc_request` accepts it (it builds the typst request with `post: compile_typst`, so the document's `format.typst` options apply), the resolver still classes it `neither`, and the host leaves it out of the menu until H8. `RequestPost` is in the job id when set, so `pdf` and `typst` requests with equal argv differ.
- `typst_available_fonts` now feeds the `typst-available-fonts` filter param through `PrepareOptions` (typst formats only) as well as being echoed, so it is in the job id.
- **Document date pin (the PDF's `/CreationDate` and `/ID`):** typst.ts has no date option, so the host prepends `typst_date_prelude(SOURCE_DATE_EPOCH)` (`#set document(date: datetime(...))`, UTC) as the first line of the `.typ` before compiling a `compile_typst` request. Verified with typst 0.14.2: two compiles of the pinned source are byte-identical and two unpinned ones are not. Not done through pandoc: `-V header-includes=` and `--include-in-header` both replace a user's `header-includes`, and a template edit would invalidate the recordings. Exported to the host as `typst_date_prelude`.
- The typst packages and Font Awesome fonts are exported by `get_typst_assets` / `get_typst_assets_version` (own SHA-256, `README.md` excluded), separate from the share tree.
- Two new warnings for the browser `.typ` request (`pandoc_request/typst_limits.rs`, read off the Pandoc JSON): `Q-20-9` remote images (a hard failure until R6's prefetch) and `Q-20-10` a raw HTML table or `<pre>` that carries a `<style>` (not CSS-inlined until R8). _R8 retired `Q-20-10` and `typst_limits.rs`: the stage inlines the styling, so there is nothing to warn about; `Q-20-9` now comes from `PrefetchRemoteImagesStage` (R6)._
- Google and URL brand fonts, the font cache and `typst fonts` are skipped in the browser (D8.6); `source: file` brand fonts reach only the `pdf` request.

## Checklist

### Tests first
- [x] A typst fixture's request argv/env/files equal the recorded native run (R0 wrapper), with the `.typ` limitations noted in the test.
- [x] A typst document with a raw HTML table: the wasm request's params carry no `quarto-cli-path` (PR #766's `inline_css` is a no-op on wasm, so the table is not CSS-inlined until R8; design D9).

### Tasks
- [x] The typst writer request: template dir/partials (`TYPST_TEMPLATE_DIR`, user template and partials as `files`), params, section numbering and heading shift, brand handling, and the `--defaults` toc yaml (`pandoc_write.rs:~893`).
- [x] Typst-only `std::fs`/`fetch_url`/`Command` sites (evidence §4) become VFS-backed or skipped, including `typst_highlight.rs`\'s `std::fs::read_to_string(doc_dir.join(style))` for a `.theme` highlight style (given a runtime parameter, since the `.theme` file is read into argv text, not mounted) and the `typst fonts`/package-extraction sites.
- [x] **Brand fonts:** `source: file` fonts come in through `resource_refs`; Google/URL brand fonts and the font cache (`font_cache_dir`, `stage_brand_fonts` in `typst_google_fonts`, which write to disk) are skipped on wasm and the limitation documented (D8.6).
- [x] Document the remote-image limitation in the response envelope's diagnostics so host H5 can report it, and the CSS-inlining limitation above (fixed by R8).
- [x] **PDF-ready request (consumed by host H8):** the `pdf` format-table entry (flagged hidden from the menu until host H8), `post: compile_typst` with `output_path = *.typ`, the vendored `TYPST_PACKAGES_DIR` and Font Awesome fonts in a separate typst-assets export with its own version (not part of the share tree docx workers mount), `typst_available_fonts` honoured when given, and the document date pinned from `source_date_epoch` for the compile (typst.ts has no date option, so the clock otherwise sets the PDF's `/CreationDate` and `/ID`). The download extension comes from the format table (the typst `Format`\'s `output_extension` is `pdf`, and `FormatIdentifier::Pdf` is deliberately not a pandoc-hybrid format, `format.rs:244`).
- [x] **Open (decided):** pandoc never reads typst images or brand fonts; they matter only to H8's compile. They go into `resource_refs` only when `post: compile_typst` (`PrepareOptions::post`): a `.typ` download mounts no images and no brand font or logo files, a `pdf` request mounts the AST's images plus every brand `source: file` font and logo (`typst_brand::brand_asset_files`). Other formats are unchanged.

## Verification

Crate-scoped clippy/nextest, the wasm compile check, the hub-client wasm test; typst fixtures through the host's parity net once H3 exists.

## Exit

A typst request equals the native recording structurally and replays through `_start` in Node; the H5 menu entry and the PDF work then have their input.

## Close-out

- [x] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [x] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [x] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: `pandoc-wasm/r4-typst-source`, workspace-6.
- Last commit; tasks ticked: all tasks and Close-out ticked; landed on `feature/pandoc-wasm`. Workspace nextest: 15588 passed, 202 skipped (R3: 15562/202; +26). Accounted for: 19 `#[test]` added and 1 replaced in this phase's files (net +18: `pandoc_request_typst` 13, the typst recording test, `typst_limits` 2, `typst_pdf` 2). The other +8 are not attributed: they are not tests this phase added, so the baseline may have moved (re-measure at R5 start). Hub-client wasm vitest: 229 passed.
- State and gotchas:
  - `typst_prestep(doc, ctx, native_effects)` is ungated; `PandocPrepareStage` passes `false`, native `PandocWriteStage::run` passes `true` (font staging and `typst fonts` live in cfg-split `typst_native_font_effects`).
  - `typst_highlight_args` takes a `&dyn SystemRuntime` (new last parameter).
  - `PrepareOptions` gained `typst_available_fonts` and `post`; every literal needed both.
  - The typst-vs-recording test reuses the docx test's body (`check_against_recordings`); it scrubs `typst-available-fonts` from both sides (the recording's came from `typst fonts`).
  - Hub-client wasm test: the real pandoc.wasm ran a typst request and produced the `.typ` (`#show: doc => article(` from the template partials).
  - The error catalog gained `Q-20-9` and `Q-20-10`; edit it as text (a JSON round trip rewrites unicode escapes).
- Next step: R5 (check its Start gate).
