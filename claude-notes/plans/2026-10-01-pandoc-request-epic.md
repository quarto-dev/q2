---
title: 'Epic: the PandocRequest seam (Rust side of pandoc.wasm in hub-client)'
date: 2026-10-01
description: 'Splits the native pandoc stage into a pure prepare step shared with wasm and a native-only execute step, so the browser can replay the same `PandocRequest` and hub output matches native output, then widens it to more formats.'
status: draft  # Planned; nothing started
---

**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md)
**Companion epic:** [`2026-10-01-pandoc-host-epic.md`](2026-10-01-pandoc-host-epic.md) (browser host, UI, PDF)
**Integration branch:** `feature/pandoc-wasm` (from `origin/main`)
**Evidence:** [`../research/2026-10-01-pandoc-wasm-evidence.md`](../research/2026-10-01-pandoc-wasm-evidence.md)

## The problem

The native `q2 render --to docx|pptx|epub|typst` path builds a pandoc invocation (argv, env, files, filter params) and runs a real `pandoc` through `PandocWriteStage`. Hub-client needs the same invocation as *data*: a pure `PandocRequest` that a browser host can replay against the official `pandoc.wasm`, so hub output equals native output for the same document. This epic splits the stage into a pure `prepare()` shared by native and wasm and a native-only `execute()`, makes the pipeline pieces it needs compile for `wasm32`, exposes the request through `wasm-quarto-hub-client`, and then widens it (code cells, typst source, pptx/epub, remote images, projects and books).

## Key architecture decisions (see the design doc for rationale)

- The Rust side is the single source of truth for argv, env, file set and filter params; native and wasm differ only in `execute()` (design: Target architecture).
- The request carries its own bytes, `/`-normalized absolute paths and a deterministic share root `/__q2_share__` (design: Contracts, Path anchoring).
- `resource_refs` is a purpose-built computation, not the resource-copy collector (D3).
- The wasm is pinned to `PANDOC_PIN` (D12); the share tree is embedded in the Rust wasm (D1).
- First slice is **docx, single document, remote images left to pandoc**; everything else is a later, separately gated phase.

## Phases

| Plan | Scope | Depends on | Novel vs. port |
|---|---|---|---|
| **R0** [foundations](2026-10-01-pandoc-request-R0-foundations.md) | Rebase onto `origin/main`; shared constants file and pin; native-input capture wrapper and extractor CLI; hashable-request exploration; Rust wasm size baseline | none | Novel tooling |
| **R1** [request seam](2026-10-01-pandoc-request-R1-request-seam.md) | Published schema and golden; request transport via `RenderContext`; `prepare()`/`execute()` split; the full `PandocRequest`; native tests | R0; **human checkpoint:** host phase H0 passed | Refactor of P4 machinery |
| **R2** [wasm enablement](2026-10-01-pandoc-request-R2-wasm-enablement.md) | wasm32 compile blockers; stage-list builder split; in-memory bundle and share tree; `wasm-quarto-hub-client` exports (request, share tree, classify, format table, resolver); resolver check; measurements | R1 | Novel exports |
| **R3** [code cells](2026-10-01-pandoc-request-R3-code-cells.md) | Capture inputs to `render_pandoc_request`; splice cached results; unexecuted-cell count | R2 | Novel (reuses preview capture splice) |
| **R4** [typst source](2026-10-01-pandoc-request-R4-typst-source.md) | The typst writer request (template, params, numbering, brand), VFS-backed or skipped OS sites; documented limits; the PDF-ready request (hidden `pdf` entry, `post: compile_typst`) for host H8 | R2 (R0 rebase for PR #766) | Port of native typst path |
| **R5** [pptx and epub](2026-10-01-pandoc-request-R5-pptx-epub.md) | In-memory `epub_extra_args`; pptx/epub fixtures and comparisons | R2 (file-serial with R4 on `pandoc_write.rs`) | Port |
| **R6** [snapshot and remote images](2026-10-01-pandoc-request-R6-snapshot-remote-images.md) | Click-time VFS snapshot; async prefetch stage and `src` rewrite; hardened fetch | R2, R3 (the prefetch follows the capture splice); R4 for the typst case | Novel |
| **R8** [CSS inlining](2026-10-01-pandoc-request-R8-css-inlining.md) | Rust-side inlining stage for typst raw HTML tables (gt, pandas), shared by native and wasm | R2, R4 | Novel, replaces PR #766's Lua-side pipe in the hub |
| **R7** [projects and books](2026-10-01-pandoc-request-R7-projects-books.md) | Pandoc `Pass2Renderer` variant; image-target normalization; active-page books (stages 0-1; the whole-book stage is R9) | R2, R4 | Novel, reverses a scoped-out preview limit |
| **R9** [whole book](2026-10-02-pandoc-request-R9-whole-book.md) | Citeproc reads bibliography/CSL through the runtime (a browser bug today); built-in extension filters (orange-book) mounted into the request's `files`, which also fixes today's chapter-alone typst/pdf book requests; ungate the pandoc pause/finishing builders and split `render_book_single_file`; chapter-relative resource mounting; wasm book driver with progress/cancel hook; whole-book typst/pdf/epub request with `scope`, per-chapter captures and `stats.book`; resolver `book` field | R7 stages 0-1, R6, R4/R5 | Novel, reverses a scoped-out preview limit |

Order: R0 → (H0 gate) → R1 → R2 → \{R3, R4, R5, R6\} → \{R7, R8\} (R8 after R4), then R9 (R7's stages 0-1 are landed; the whole-book work is R9). R3-R6 can proceed in parallel worktrees except that R4, R5 and R6 all edit `pandoc_write.rs`, and R3, R6, R7 and R8 all touch the `wasm-quarto-hub-client` `lib.rs`, the `.d.ts`, `wasmRenderer.ts` and `build_pandoc_prefix_stages`, so merge them one at a time. R6's prefetch runs after R3's capture splice, and R6's typst test needs R4. The H5 demo is a human checkpoint before R3-R8 are wired into the menu, not a gate on the request phases. Lane R (workspace-6) runs this epic in the order above and lane H (workspace-7) the host epic; the design's Parallel development gives the lanes, the landing points and the hand-offs between them. Each phase is one agent.

## Progress

Tick a phase when its Close-out is complete and it has landed on `feature/pandoc-wasm`.

- [x] R0 [foundations](2026-10-01-pandoc-request-R0-foundations.md)
- [x] R1 [request seam](2026-10-01-pandoc-request-R1-request-seam.md)
- [x] R2 [wasm enablement](2026-10-01-pandoc-request-R2-wasm-enablement.md)
- [x] R3 [code cells](2026-10-01-pandoc-request-R3-code-cells.md)
- [x] R4 [typst source](2026-10-01-pandoc-request-R4-typst-source.md)
- [x] R5 [pptx and epub](2026-10-01-pandoc-request-R5-pptx-epub.md)
- [x] R6 [snapshot and remote images](2026-10-01-pandoc-request-R6-snapshot-remote-images.md)
- [x] R8 [CSS inlining](2026-10-01-pandoc-request-R8-css-inlining.md)
- [x] R7 [projects and books (stages 0-1)](2026-10-01-pandoc-request-R7-projects-books.md)
- [ ] R9 [whole book](2026-10-02-pandoc-request-R9-whole-book.md)

## Dependencies on the host epic

| This epic needs | From | For |
|---|---|---|
| Captured native inputs consumed, Lua gate passed | H0 | R1 start (human checkpoint) |
| Request schema and golden consumed | H1 | published by R1's first task, read by H1 |
| `render_pandoc_request`, share-tree export, classify export, format table, resolver | consumed by H3, H5 | produced by R2 |
| Soft: host harness and parity net for typst/pptx/epub/remote-image checks | H3 | R4, R5, R6 (run once H3 exists) |

## Definition of done

- [ ] A docx request produced by `prepare()` natively equals what the native pandoc run used (argv, env keys, file set), and the wasm build exposes the same request to the browser.
- [ ] pptx, epub and typst-source requests, remote images (including typst), code-cell results and project pages produce correct requests; books follow the staged direction in R7 and R9.
- [ ] `cargo check --target wasm32-unknown-unknown` passes in `crates/wasm-quarto-hub-client` and `npm run build:wasm` succeeds; native behaviour and goldens are unchanged.

## Known limitations (v1)

- The `.typ` download bundles no images or brand logos, and `typst-available-fonts` is unset (design D8.6).
- A typst document with a remote image fails until R6 lands the prefetch (D5).
- Books render the active page only until R9 (typst, pdf, epub: the whole book; docx and pptx stay active-page-only by design).
- A user filter at the project root cannot `require` or read any other file in the hub (only the filter itself mounts, for performance and because the root holds unrelated files); put a family of filters in a subdirectory.
- `prepare()` runs on the browser main thread and blocks the UI for large documents.
- User filters in the default position run in pampa's Lua; post-position and entry-point filters run in pandoc.wasm, where the working directory is `/`, only the request's environment is visible and files beside the document are not mounted (design D9).
- New native tests must pass on Windows although CI does not run it; the pure normalizer tests are the guard.
