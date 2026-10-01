# Plan: Projects and books (pandoc-request R7)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-request-epic.md`](2026-10-01-pandoc-request-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (D3, D10)
**Depends on:** R2, R4. **Unblocks:** project downloads in host H5. Stages 2-3 (whole-book) are epic-sized and get their own plan; this phase's v1 scope is stages 0-1.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** R (workspace-6). **Start gate:** R2 and R4 are fully ticked. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** none; lane R is done (book stages 2-3 need their own plan first).

## Overview

Multi-file projects take a different render path from single documents, and books that consolidate into one file (Typst, EPUB) should ideally render as a whole with the active chapter located in the result. This phase adds the pandoc `Pass2Renderer` variant, normalizes image targets against native, and stages the whole-book direction. User filters and extensions (installed in `_extensions/<name>/`) mount through `resource_refs` (R2) as project files; a project fixture with a user filter and an installed extension covers this.

## Decisions

- A project download renders the *active page* until the book stages below land, and the UI says so.
- The whole-book direction reverses the book-projects design doc's scoping-out of single-file-merge preview (`designs/book-projects-architecture.md` §11) for the download/PDF path only.

## Checklist

### Tests first
- [ ] A project with `format: html` where docx is requested: the request's `writer` is `docx` (re-detection must not override the format).
- [ ] A project-level `format: docx: reference-doc:` with the document in a subdirectory: the path resolves against the declaring file, so `resource_refs` keys on the resolved path, not on `doc_dir`.
- [ ] Image-target equality against native for a project with `output-dir == dir`, including a site-root image (`/img/a.png`: vfs-root mode leaves it as written, so the request must resolve it against the project root); projects with `output-dir != dir` are excluded or normalized (the wasm prelude's vfs-root mode leaves `Image.target` doc-relative, `link_rewrite.rs:~285`, whereas native website mode relativizes to the output page).
- [ ] A book fixture: active-page smoke test (no native baseline to compare). Native rejects docx and pptx for books with `Q-5-33`, whereas the wasm path calls plain `run()` and `install_book_chapter_seeds` is a no-op there, so mirror the rejection or document the difference.

### Tasks
- [ ] **Stage 0 — pandoc `Pass2Renderer`:** add `format_override: Option<&str>` to `render_project_active_page_to_response` (`lib.rs:1699`); when set, build the `Format` from it and skip `detect_format_from_content` and the preview-format mapping (`lib.rs:~1731-1740`), as `render_single_doc_to_response` does (`:1532-1546`). `ProjectPipeline::with_format_override` (`orchestrator.rs:998`) is native-only and is not reused. Add a `RenderToPandocRequestRenderer` in `pass2_renderer.rs` with its own `Output` type (`source_path`, `request`, `diagnostics`, `source_context`, `document_profile`), not a `Pass2Payload` variant: the wasm tail's exhaustive payload match builds a JSON `RenderResponse`, which cannot carry `Uint8Array`s. The trait's `render()` has no room for `PrepareOptions`, `typst_available_fonts`, captures (R3) or the snapshot (R6), so they are renderer fields set by its constructor. `render()` builds its own context as `RenderToHtmlRenderer::render` does (sets `ctx.project_index` and the vfs-root resolver), runs R2's `quarto-core` render function and takes `ctx.pandoc_request`; it also needs a synthetic `output_path` that cannot collide with `output-dir`, and emits the `Q-5-12` "render scripts not run" warning. A sibling export re-discovers the project as `lib.rs:~1334` does and builds R2's bytes envelope, returning early rather than through the JSON tail.
- [ ] **Image-target normalization** against native (design D10).
- [ ] **Books, stage 1 — active page:** the active page of a book renders alone (smoke test only).
- [ ] **Books, stage 2 — whole-book request for Typst (own plan; not v1).** Native `run_with_book_support` (`project/orchestrator.rs:1013`) renders every chapter through the pause stages to Normalization, merges the chapter bodies (`project/book/merge.rs`, which stamps each chapter's heading with `quarto-book-item-file`/`-number`/`-depth`), then runs the finishing stages from Crossref and the unchanged pandoc tail (`pipeline.rs:603-677`). That machinery is native-only today (`project/book/mod.rs:16,24`; the pandoc pause/finishing builders at `pipeline.rs:~655/671`). A wasm book orchestrator (ungate the pause/finishing builders and `single_file_render`, gated at `project/book/mod.rs:24`; `merge.rs` is already ungated; drive every chapter through the wasm runtime) produces one whole-book request.
- [ ] **Books, stage 3 — chapter location (own plan; not v1):** the chapter being edited is located in the output by its `quarto-book-item-file` heading attribute (for Typst, an anchor/label per chapter emitted from it, which edits the vendored `typst.lua`; for PDF, host H9 navigates by that anchor).

Each stage is useful on its own; stages 2-3 need their own plan before they start.

## Verification

Crate-scoped clippy/nextest, the wasm compile check, the hub-client wasm test; one workspace nextest at phase end.

## Exit

Project pages (and, as far as stages 2-3 land, whole books) download correctly.

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
