# Plan: Flush resource copies before `TypstCompileStage` runs (book-projects P2c)

**Date:** 2026-09-24
**Epic:** [`2026-09-21-book-projects-epic.md`](2026-09-21-book-projects-epic.md) (new phase, inserted between P2b and P3 — see the epic's phase table)
**Depends on:** none directly, but discovered while scoping P2's item 49 (figure-numbering integration test needs a compiling book with images).
**Blocks:** P2's item 49 (and any other P2 item whose fixture needs a compiling Typst document with a local image reference).

## Overview

Investigating why a merged single-file-book Typst render with a chapter
image (`![](img.png)`) fails to compile (`typst compile` error: `file not
found` at `_book/img.png`) found a bug **broader than book-projects**: any
Pandoc-hybrid Typst-compile render — book or ordinary single document —
whose output directory differs from the source directory and references a
local image fails identically. Confirmed empirically with a plain,
non-book single-document project (`project: {type: default, output-dir:
_output}`, one `.qmd` with a markdown image): the same `file not found`
error at `_output/img.png`.

**Root cause.** `render_document_to_file` runs the whole render — including,
for a Typst target, `TypstCompileStage` — as one `Pipeline::run` pass
(`build_pandoc_pipeline_stages` appends `PandocWriteStage` then
`TypstCompileStage`, in that order, as the tail of a single stage list).
Only *after* that entire pass returns does `render_document_to_file` call
`finalize_rendered_output`, which is what actually drains
`ctx.resource_copies` (populated earlier by `ResourceCollectorTransform`,
an ordinary AST transform) through a real `OutputSink` and physically
copies the referenced image bytes to the output directory. So the real
`typst compile` subprocess — which reads the intermediate `.typ` file's
image references directly off disk, relative to the output directory —
runs and fails *before* the copy that would have supplied the image ever
executes.

docx/pptx never hit this: pandoc's own writer embeds a referenced image's
bytes directly into the `.docx`/`.pptx` at *write* time, reading from the
image's already-resolved source path — it never needs the image to exist
at the output directory first. Typst is the only Pandoc-hybrid format with
a second, later, disk-reading subprocess.

**The fix is ordering, not new copy logic.** Insert a new native-only
pipeline stage, `ResourceCopyFlushStage`, between `PandocWriteStage` and
`TypstCompileStage` in `build_pandoc_pipeline_stages` — it performs the
exact same drain-and-copy `finalize_rendered_output` already does (same
`OutputSink`/`enqueue_resource_copies` machinery, so the bd-cfl67
allowed-roots validation is preserved, not bypassed), just early enough for
`typst compile` to see the file. `finalize_rendered_output`'s own drain
becomes a no-op afterward (`std::mem::take` already emptied
`ctx.resource_copies`), so nothing is copied twice. Because
`build_pandoc_pipeline_finishing_stages` (the book-merge tail) derives from
the same `build_pandoc_pipeline_stages` list (`finishing_stages_from`),
this one change point fixes both the ordinary single-document path and the
book single-file-merge path.

## Checklist

### Tests first
- [x] Integration test (RED against current code, confirmed): a plain,
      non-book `type: default` project with `output-dir: _output`, one
      `.qmd` referencing a local markdown image, rendered `--to typst`,
      produces a real compiled PDF **and** the image lands at
      `_output/img/dot.png` — not just "render succeeds," the actual file
      is inspected.
      `typst_project_copies_image_to_output_dir_before_compiling`
      (`crates/quarto-core/tests/integration/pandoc_typst_resource_copy.rs`).
      RED confirmed with the exact production error (`Q-21-3`: `file not
      found (searched at .../_output/img/dot.png)`) before the fix; a bare
      `render_document_to_file` call with no `_quarto.yml` does **not**
      reproduce it (a synthetic single-file "project" computes a
      climb-back-to-source relative image path instead) — this test must
      go through a real `ProjectPipeline` render, same as `q2 render
      <project>`.
- [x] Book single-file-merge case: **deferred to P2 item 49**, not
      duplicated here. Item 49's own fixture (3 chapters with figures,
      compiled and inspected for numbering) already needs a real image and
      exercises `build_pandoc_pipeline_finishing_stages`, which derives
      from the exact same `build_pandoc_pipeline_stages` list this phase
      fixes — so item 49 is itself this phase's book-path regression guard
      once written, with no separate test needed now.
- [x] Regression test / negative control: docx keeps working exactly as
      before. Not a new test — an **existing** one already covers this
      shape and stayed green throughout:
      `render_document_to_file_docx_embeds_a_relatively_referenced_image`
      (`pandoc_render_to_file.rs`) — pandoc's own writer embeds the image
      directly at write time; `ResourceCopyFlushStage` is Typst-only
      (guarded the same way `TypstCompileStage` is), so docx/pptx never
      reach it.
- [x] Unit test: `ResourceCopyFlushStage` with an empty `ctx.resource_copies`
      is a no-op passthrough. `empty_resource_copies_is_passthrough`.
- [x] Unit test: `ResourceCopyFlushStage` drains `ctx.resource_copies` to
      empty after running. `drains_resource_copies_to_empty`.

### Implementation
- [x] `ResourceCopyFlushStage` (`crates/quarto-core/src/stage/stages/
      resource_copy_flush.rs`), native-only like `PandocWriteStage`/
      `TypstCompileStage`. Input/output kind `RenderedOutput` (sits between
      `PandocWriteStage` and `TypstCompileStage`, passes the value through
      unchanged). Drains `ctx.resource_copies` via `std::mem::take`,
      constructs an `OutputSink` from `ctx.resource_resolver`'s
      `allowed_output_roots()`, calls the existing
      `resource_copy_diagnostics::enqueue_resource_copies` +
      `OutputSink::flush` — the same two calls `finalize_rendered_output`
      already makes, just earlier and in their own stage.
- [x] Wire into `build_pandoc_pipeline_stages` (`pipeline.rs`): pushed
      `ResourceCopyFlushStage` right before the conditional
      `TypstCompileStage` push, guarded the same way (`format_identifier ==
      Typst`) — docx/pptx get no new stage, matching that they never needed
      one. Two pre-existing pipeline unit tests asserted the exact stage-
      list tail shape and needed updating for the new member:
      `pandoc_finishing_stage_list_is_bounded_transforms_then_real_tail`,
      `typst_stage_list_appends_typst_compile_after_pandoc_write`.
- [x] Register the new module in `stage/stages/mod.rs` (native-only `mod`/
      `pub use`, mirroring `typst_compile`'s registration).
- [x] Re-run and confirm GREEN: the new tests above, plus the full
      `pandoc_typst_*`/`book_single_file_merge*`/`pandoc_render_to_file`/
      `resource_copy*`/`resource_report*` suites for regressions (34 tests,
      all green).
- [x] `cargo clippy -p quarto-core --all-targets -- -D warnings` — clean.
      `cargo nextest run -p quarto-core --no-fail-fast`: **4980 passed (1
      slow), 31 skipped, 0 failed** — delta from the P2b-landing baseline
      (4976 passed) is exactly `+4` (3 new unit tests in
      `resource_copy_flush.rs`, 1 new integration test), confirmed by
      `git diff --stat` showing every changed file under
      `crates/quarto-core/`. Phase-boundary `cargo nextest run --workspace`:
      **14832 passed (1 slow), 200 skipped, 0 failed** — exactly the same
      `+4` delta from the P2b-landing workspace baseline (14828 passed).

## Details

**Why this isn't itemized in P2's own plan.** P2's item 49 assumed a
3-chapter book with figures could simply be rendered and compiled once the
dispatch gap (P2b) was fixed. That assumption held for numbering logic but
not for asset placement — a genuinely separate, pre-existing bug in the
Pandoc-hybrid/Typst pipeline's stage ordering that happens to have been
invisible until the first real fixture with both an `output-dir` and a
local image was attempted.

**Why this affects more than book-projects.** Confirmed directly: a plain
non-book Typst render with `output-dir` set and a local image reference
fails identically. Any Quarto 2 Typst project with images and a
project-level `output-dir` is broken today, independent of book-projects.
