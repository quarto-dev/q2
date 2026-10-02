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
- [x] A project with `format: html` where docx is requested: the request's `writer` is `docx` (re-detection must not override the format).
- [x] A project-level `format: docx: reference-doc:` with the document in a subdirectory: the path resolves against the declaring file, so `resource_refs` keys on the resolved path, not on `doc_dir`.
- [x] Image-target equality against native for a project with `output-dir == dir`, including a site-root image (`/img/a.png`: vfs-root mode leaves it as written, so the request must resolve it against the project root); projects with `output-dir != dir` are excluded or normalized (the wasm prelude's vfs-root mode leaves `Image.target` doc-relative, `link_rewrite.rs:~285`, whereas native website mode relativizes to the output page).
- [x] A book fixture: active-page smoke test (no native baseline to compare). Native rejects docx and pptx for books with `Q-5-33`, whereas the wasm path calls plain `run()` and `install_book_chapter_seeds` is a no-op there, so mirror the rejection or document the difference.

### Tasks
- [x] **Stage 0 — pandoc `Pass2Renderer`:** add `format_override: Option<&str>` to `render_project_active_page_to_response` (`lib.rs:1699`); when set, build the `Format` from it and skip `detect_format_from_content` and the preview-format mapping (`lib.rs:~1731-1740`), as `render_single_doc_to_response` does (`:1532-1546`). `ProjectPipeline::with_format_override` (`orchestrator.rs:998`) is native-only and is not reused. Add a `RenderToPandocRequestRenderer` in `pass2_renderer.rs` with its own `Output` type (`source_path`, `request`, `diagnostics`, `source_context`, `document_profile`), not a `Pass2Payload` variant: the wasm tail's exhaustive payload match builds a JSON `RenderResponse`, which cannot carry `Uint8Array`s. The trait's `render()` has no room for `PrepareOptions`, `typst_available_fonts`, captures (R3) or the snapshot (R6), so they are renderer fields set by its constructor. `render()` builds its own context as `RenderToHtmlRenderer::render` does (sets `ctx.project_index` and the vfs-root resolver), runs R2's `quarto-core` render function and takes `ctx.pandoc_request`; it also needs a synthetic `output_path` that cannot collide with `output-dir`, and emits the `Q-5-12` "render scripts not run" warning. A sibling export re-discovers the project as `lib.rs:~1334` does and builds R2's bytes envelope, returning early rather than through the JSON tail.
- [x] **Image-target normalization** against native (design D10).
- [x] **Books, stage 1 — active page:** the active page of a book renders alone (smoke test only).
- [ ] **Books, stage 2 — whole-book request for Typst (own plan; not v1).** Native `run_with_book_support` (`project/orchestrator.rs:1013`) renders every chapter through the pause stages to Normalization, merges the chapter bodies (`project/book/merge.rs`, which stamps each chapter's heading with `quarto-book-item-file`/`-number`/`-depth`), then runs the finishing stages from Crossref and the unchanged pandoc tail (`pipeline.rs:603-677`). That machinery is native-only today (`project/book/mod.rs:16,24`; the pandoc pause/finishing builders at `pipeline.rs:~655/671`). A wasm book orchestrator (ungate the pause/finishing builders and `single_file_render`, gated at `project/book/mod.rs:24`; `merge.rs` is already ungated; drive every chapter through the wasm runtime) produces one whole-book request.
- [ ] **Books, stage 3 — chapter location (own plan; not v1):** the chapter being edited is located in the output by its `quarto-book-item-file` heading attribute (for Typst, an anchor/label per chapter emitted from it, which edits the vendored `typst.lua`; for PDF, host H9 navigates by that anchor).

Each stage is useful on its own; stages 2-3 need their own plan before they start.

## Verification

Crate-scoped clippy/nextest, the wasm compile check, the hub-client wasm test; one workspace nextest at phase end.

## Exit

Project pages (and, as far as stages 2-3 land, whole books) download correctly.

## Close-out

- [x] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [x] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [x] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: `pandoc-wasm/r7-projects-books` in `.worktrees/workspace-6`, cut from `feature/pandoc-wasm` (`bbe5c3a2c`).
- Live workspace baseline at R7 start (measured on the cut, before any edit): 15615 run, 15615 passed, 202 skipped (matches R8's close-out).
- Stage 0 and image normalization done, one commit. **Deviations from the task text, all deliberate:** (1) no `format_override` parameter on `render_project_active_page_to_response`, and no sibling export. `render_pandoc_request` already carries the requested format, so the project path lives in `quarto-core` (`pandoc_request::render::render_project_request`, called where the `!project.is_single_file` refusal was) and the existing export serves both cases: no `.d.ts`, `wasmRenderer.ts` or host change, and the project path is testable natively. `render_project_active_page_to_response` is untouched (it is the preview path and detects the format on purpose). (2) `RenderToPandocRequestRenderer` (`project/pass2_renderer.rs`) has output type `PandocRequestPassTwoOutput { source_path, outcome: PandocRequestOutcome, document_profile }`; the request, diagnostics, source context and unexecuted-cell count are the outcome's fields. The renderer holds `PrepareOptions` and the captures. The per-context half of `render_pandoc_request` is now `build_request_in_context`, shared by the single-document path and the renderer, so a project page runs exactly the single-document stages.
- Image targets: the renderer ignores the hub's vfs-root resolver and installs a website-mode resolver rooted at `project.dir` with a synthetic `output_path` (`<project dir>/<source's relative path>.<ext>`, nothing written). Native relativizes an image to the output page, and a page that mirrors the source layout under any root gives the same string, so the result equals native's for `output-dir == dir` **and** for `output-dir != dir` (both are tests): `/img/a.png` from `sub/page.qmd` becomes `../img/a.png`, which `ResourceCollector::add_image` then resolves against the document directory. With the vfs-root resolver the site-root image stayed `/img/a.png` and was refused as outside the project (mutation-checked: the two equality tests fail with it). The single-document path keeps the hub's resolver (site root == page directory there).
- `Q-5-12` (project render scripts not run): the wasm wrapper adds it to the diagnostics of every project download, bypassing the preview's once-per-session gate, since a download's diagnostics are its whole report (vitest-covered; not native-testable because `RenderHost` handling lives in the wasm crate).
- Native-only side effect to know about: the orchestrator's `run_inner` runs resource copy and writes `.quarto/render-manifest.json` under `#[cfg(not(wasm32))]`, so the native tests of the project path write that file into their scratch project; the wasm path does not.
- Gates so far: `quarto-core` clippy `-D warnings` clean, nextest 5501 passed, 32 skipped; wasm `cargo check` clean (the crate is edition 2021: no let-chains there); `npm run build:wasm` ok; `pandocRequest.wasm.test.ts` 35 passed (3 new: project page with image normalization, `Q-5-12` on every download, book chapter alone). Removed the two tests that asserted the old "projects not yet supported" refusal (native `pandoc_request_exports.rs`, vitest).
- **Q-5-33 decided (Gordon):** the rule is native's, about books (docx/pptx have no single-file book merge). The browser path does not mirror it: a chapter download is the active page alone, like the html preview, for every format, **silently** (no warning diagnostic). Pinned by `a_book_chapter_downloads_as_docx_and_pptx_without_native_rejection_or_warning`; the typst chapter smoke test (`a_book_chapter_renders_alone_as_the_active_page`) and a vitest chapter case cover the rest. The difference from native is documented here and in the wasm export's doc comment. Revisit with stage 2 (whole-book request), which needs its own plan.
- Next step: close-out (workspace nextest, reconcile, rebase, land). Stages 2-3 stay unticked: own plan first.
- **Close-out (R7 stages 0-1 complete; lane R is done).** Phase-end workspace nextest on the final tree (before the rebase onto H7, which touched no Rust): 15625 run, 15625 passed, 202 skipped. Against the live start baseline (15615 / 202) that is +10 passed, 0 skipped: +11 new tests in `tests/integration/pandoc_request_projects.rs` (html-project docx writer; document-level `format: html`; project and document `reference-doc`; image targets equal native for `output-dir == dir` and `!= dir`; website project; document with errors; sibling with errors; book chapter typst; book chapter docx/pptx) minus the 1 removed refusal test in `pandoc_request_exports.rs`. Other gates: `quarto-core` clippy `-D warnings` clean, nextest 5501 + the later book test; wasm `cargo check` clean; `npm run build:wasm`; `npm run test:wasm` 248 passed, 28 skipped (R8 recorded 240 / 28; this phase's net is +2, 3 new cases minus the removed refusal case; the other +6 are not attributed: no hub-client test file other than `pandocRequest.wasm.test.ts` differs from the cut, so most likely the handoff's 240 predates a count that R8 did not re-measure). Checklist reconciled against the code and tests: stages 0, normalization and book stage 1 are ticked and landed with tests; **book stages 2 and 3 are deliberately unticked** (not v1: each needs its own plan, noted in the file's Next in lane). Q-5-33 resolved by Gordon (see above); menu wiring remains gated on the H5 demo checkpoint (human review), which this request-side phase does not need.
- Next in lane: none; lane R is done. Open for Gordon: the H5 demo checkpoint gates wiring project downloads into the menu; the whole-book plan (stages 2-3) if wanted.
