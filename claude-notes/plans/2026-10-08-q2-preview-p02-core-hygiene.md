# Plan 2: core hygiene: image resolution outside the source tree, and the temp-dir leak

Part of the epic [`2026-10-08-q2-preview-hub-parity-epic.md`](2026-10-08-q2-preview-hub-parity-epic.md) (E9, E10); the execution model and landing procedure are there.

## Execution

- **Lane:** B (`workspace-2`), binding: one agent per plan, in this order: Plan 2, Plan 6, Plan 9, Plan 12. This plan is number 1 of 4 in the lane.
- **Waits on:** nothing. Nothing inside this plan waits on another plan.
- **Start gate:** none; start immediately.
- **Branch:** `q2-preview/p02-core-hygiene`, cut from the tip of `feature/q2-preview-hub-parity` when you start.
- **Merges to:** `feature/q2-preview-hub-parity`, by the landing procedure in the epic.
- **Unlocks:** Plan 6 (typst render foundations: capture plots need the image fix).
- **Next in lane:** Plan 6 (`2026-10-08-q2-preview-p06-typst-render-foundations`); the next agent starts only when its own start gate passes.

## Goal

Two defects in the shared pipeline, both visible today without any new feature: (1) a render whose output directory is outside the source tree loses its images (docx, odt, epub from `/api/preview/render`; typst capture plots); (2) every native render leaks a temp directory.

## Background facts

- **Image resolution under an output dir outside the source tree.** `LinkRewriteTransform` rewrites every image target relative to the output page (`transforms/link_rewrite.rs:285-297`, `resolve_static_resource_href`), and the resolver is built with `site_root = project.output_dir` while the page sits in a temp dir (`render_to_file.rs:391`). So `pic.png` becomes `../../../../../../Users/.../proj/sub/pic.png`. The resource collector joins that onto the source dir (`resource_collector.rs:83`), giving the spurious `Q-5-6`, and pandoc, given only `--resource-path <doc_dir>` (`pandoc_write.rs:1289`), cannot find it either. Reproduced at HEAD 6916c6174: docx and epub from the existing `/render` route come back without the image (ran); odt loses `Pictures/` the same way (to re-confirm in 2.2); for typst in a default project (`output_dir == dir`) the `.typ` in `<proj>/.quarto/preview-out/x` uses `../../../sub/pic.png` and builds, with only a spurious `Q-5-6` (ran). In a `type: website` or book project (`output_dir = _site`) the same render fails: the resolver's `site_root = project.output_dir` assumes the page was flushed into `_site`, so the `.typ` holds `image("../../../_site/sub/pic.png")` and typst reports `Q-21-3 file not found` (ran).
- **The rewrite is load-bearing.** Site-root-relative `![](/top.png)` works only through it; book chapter images are re-anchored by `project/book/images.rs:5-20` (covered by `tests/integration/book_chapter_images.rs`); `link_rewrite.rs:941` and `:1031` pin it for docx. Skipping it for pandoc profiles was the original plan and is wrong.
- `q2 render sub/a.qmd --to docx --output-dir <abs>` keeps images only by accident: the `../` run clamps at `/` because the output dir is deeper than the source dir. A relative `--output-dir ../x` fails with a doubled path `proj/Users/.../pic.png` for docx (ran), and for typst with "input file not found" (ran); both come from the directory not being absolutised against the cwd.
- **Leak.** `StageContext::temp_dir` calls `TempDir::into_path()` (`stage/context.rs:488-496`; the type is `quarto_system_runtime::TempDir`, `traits.rs:225`, whose `Drop` uses `std::fs` directly, not tempfile's), so every native render leaks `$TMPDIR/quarto-pipeline_*` (4.4 MB per typst render). Users: `typst_compile.rs:115`, `pandoc_prepare.rs:56`, `pandoc_write.rs:378`, `engine_execution.rs:548`; all use the path inside the pipeline, and `finish_pipeline_result` drops the `StageContext` after the last consumer. `StageContext.temp_dir` is an `OnceLock<PathBuf>` (`context.rs:79`) that `with_temp_dir(PathBuf)` also seeds with a caller-owned path (`pipeline.rs:1123`), so it cannot simply become `OnceLock<TempDir>`. `StageContext` is not `Clone`. The comment at `pandoc_filters/bundle.rs:21-22` claiming cleanup is false. This machine's `$TMPDIR` holds about 46k such directories: never glob it; count with `find ... | wc -l`. The temp dir also holds the vendored `typst-packages`, the `--package-cache-path` that `typst_gather::gather_packages` writes into (`typst_compile.rs:115-128,168`), so it cannot be shared across renders; extracting vendored packages once per process is declined.
- `pandoc_write` keeps the temp JSON on failure "for debugging" (`retain_temp_json_unless_success`, `pandoc_write.rs:~68`).

## Decisions

- **P2-D1 Mirror, do not skip.** For every format rendered with an output directory outside the source tree (docx, odt, epub, pptx and typst), the image rewrite is computed against a source-mirroring resolver: `site_root = project.dir` and `page_output` = the mirror of the source path under the project, so every rewritten href is correct relative to the source whatever the project type. For typst a path is also left alone when the file exists in the request directory (capture plots).
- **P2-D2 Leak fix in the shared pipeline.** `StageContext` owns its `TempDir` in a second field; the caller-owned path keeps working.

## Tasks

Per task: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus `cargo nextest run -p quarto-core`. Workspace nextest at the end of the plan. Integration tests go in `crates/quarto-core/tests/integration/<name>.rs` plus `pub mod` in `main.rs`. Tests needing `typst` or `pandoc` skip with a clear reason when absent.

- [ ] 2.1 Baseline `cargo nextest run --workspace` on the integration tip; record pass/skip counts.
- [ ] 2.2 Spike (was 1.1.B): reproduce the image loss at HEAD for docx, odt, epub (and typst) with a relative image into an OS temp dir and into `<proj>/.quarto/preview-out/x`; record the result here before building.
- [ ] 2.3 Image fix (was 1.2.4), first: apply P2-D1 in `render_to_file.rs` (`:391`); `RenderToFileOptions` gains `capture_materialize_dir: Option<PathBuf>` (default `None`; Plan 6 fills it), which the pandoc stage adds to pandoc's search path as an additional `--resource-path` flag next to the source directory (repeat the flag rather than joining with `:`, which breaks on Windows; pandoc accepts repeats, ran). For typst, additionally leave an image path alone when the file exists in the request directory. Absolutise `--output-dir` against the cwd where `q2 render` receives it. The same resolver removes the spurious `Q-5-6`. Tests: docx, odt and epub into an OS temp dir and into `<proj>/.quarto/preview-out/x` contain the image bytes (assert the media entry in the zip) and emit no `Q-5-6`/`Q-11-1`; `![](/top.png)` still works; the book chapter-image and `link_rewrite` tests stay green; typst finds a source image, in a default project and in a `type: website` project with a subfolder document (the second fails with `Q-21-3` today); `q2 render --output-dir` keeps images for both an absolute and a relative directory, for docx and typst (the relative case fails today; the absolute one passes only by clamping, so it is not evidence); snapshot tests containing image hrefs are checked for HTML-adjacent profiles (grep the snapshots).
- [ ] 2.4 Leak fix (was 0.3): add `owned_temp_dir: OnceLock<quarto_system_runtime::TempDir>` set by the lazy path in place of `into_path()`; audit the test mock runtimes that return real-looking paths (`document_profile.rs:298` returns `/tmp/test`) so dropping a `TempDir` can never delete a real directory; fix the false comment at `bundle.rs:21-22`; decide the failure-JSON retention (copy it out on failure, or drop the retention and any error text that cites it). First step: check that no long-lived engine process keeps files under `temp_dir` after a render ends. Test: after a `format: typst` render no `quarto-pipeline_*` directory remains (also covers `q2 render`); the test sets a private `TMPDIR` for its own process (`NativeRuntime::temp_dir` honours it), which is safe only because nextest runs each test in its own process (say so in a comment), and never counts the real `$TMPDIR`.
- [ ] 2.5 `cargo build -p quarto` and `cargo build --workspace` (struct-literal fallout from the new `RenderToFileOptions` field; `StageContext` is only built through `new()`).
- [ ] 2.6 Plan end: workspace nextest against 2.1; reconcile this checklist; write the Handoff log; land per the epic.

## Risks

- The image fix touches HTML-adjacent link rewriting: the pinned tests and snapshots are the guard; run them before and after.
- Failure-JSON retention vanishing with the context may surprise anyone debugging pandoc failures; 2.4 decides it explicitly.

## Handoff log

Written by the implementing agent in the plan-end task, committed with the checklist: what landed (commit range), any task that deviated from the plan and why, names and signatures of the interfaces later plans consume exactly as they ended up, the workspace nextest baseline and final counts, and anything the next agent in the lane or a plan that waits on this one must know. Empty until the plan lands.
