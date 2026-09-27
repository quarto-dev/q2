# P6 — Book-project-fixture harness

**Date:** 2026-09-27
**Epic:** [`2026-09-27-typst-smoke-all-epic.md`](2026-09-27-typst-smoke-all-epic.md)
**Depends on:** P1.
**Worktree:** `workspace-5` (Track B — see epic's "Parallel development plan").

## Worktree & git workflow

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3
git show feature/typst-testing:claude-notes/plans/2026-09-27-typst-smoke-all-epic-P1-assertion-vocabulary.md \
  | grep -q '^Complete\.' && echo "P1 merged" || echo "STOP: P1 not yet merged, wait"
```

Once it prints "merged":

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5
git checkout -B typst-testing/p6-book-fixture-harness feature/typst-testing
```

Implement the checklist below, gating on `cargo clippy -p quarto-test -p quarto
--all-targets -- -D warnings` + `cargo nextest run -p quarto-test -p quarto`. When
done, flip this doc's checklist to `[x]` and `## Status` to `Complete.`, commit,
then:

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5
git rebase feature/typst-testing
cd /Users/gordon/src/q2/.worktrees/workspace-3
git checkout feature/typst-testing
git merge --ff-only typst-testing/p6-book-fixture-harness   # retry from rebase (in workspace-5) if not a fast-forward
cargo nextest run --workspace                  # phase-boundary gate
```

## Scope

Today, `crates/quarto/tests/integration/smoke_all.rs:29-33` walks for `*.qmd` and calls
`run_test_file` (→ `crates/quarto-test/src/runner.rs:277` `render_document`, which
calls `quarto_core::render_to_file::render_to_file` — a **single-file** entry point) on
each file independently. There is no project-fixture concept anywhere in
`crates/quarto-test/src/`.

`override-orange-book`, `orange-book-lang`, and `orange-book`'s non-index chapters
don't need this (single-file-in-project already works, or they carry
`_quarto.tests.run.skip`, already supported per `spec.rs:24,55-57,89`). What needs
building: **whole-book rendering**, triggered by `render-project: true` in a file's
front matter, matching Q1's own semantics exactly
(`external-sources/quarto-cli/tests/smoke/smoke-all.test.ts:443-455`): resolve the
file's containing project root; if `render-project: true` and that project hasn't been
rendered yet this test run, pre-render the whole project once (deduped across every
file in that project) via the real book pipeline before evaluating that file's own
assertions.

The real book-render entry point the CLI uses: `crates/quarto/src/commands/render.rs:1050-1067`
dispatches through `crate::project::book::single_file_render::render_book_single_file`
→ `merge_book_chapters`, calling `ProjectPipeline::new(...).with_fail_fast(...).run_with_book_support()`
(async, `pollster::block_on`).

## Q2's merged-book output path (confirmed during plan review, no render needed)

`ProjectKind::Book` defaults `output_dir` to `dir/_book`
(`crates/quarto-core/src/project/mod.rs:75-83`, explicitly modeled on Q1's `book.ts`
`outputDir: "_book"`). The stem comes from `book_output_stem()`
(`crates/quarto-core/src/project/book/config.rs:419-429`: `book.output-file` →
`book.title` → project-dir-name, sanitized). Real convention:
**`<project_dir>/_book/<stem>.<ext>`** — matches Q1's `_book/<Title>.pdf` closely.
`render_book_single_file` (`single_file_render.rs`) returns this path via the same
`RenderToFileResult.output_path` field every single-document render returns
(`finalize_rendered_output`), so nothing book-specific needs inventing to *get* the
path — see "The entry point" below for how `quarto-test` actually reaches it.

## The entry point (resolved by reading code — no spike needed)

An earlier pass through this plan worried that `render_book_single_file` is
`pub(crate)` and might need a visibility change for `quarto-test` to call it. That
worry doesn't apply: `quarto-test` never needs `render_book_single_file` directly.
The real entry point is one level up — `ProjectPipeline` is `pub struct`
(`orchestrator.rs:922`), `ProjectPipeline::new` is `pub`, and
`run_with_book_support()` is `pub async fn` (`orchestrator.rs:1013`) — already used
cross-crate exactly the way `quarto-test` would need it, from
`crates/quarto/src/commands/render.rs:1050,1067` (and again at `:1238,1260`). No
visibility changes anywhere; `render_project_document()` can call
`ProjectPipeline::new(...).run_with_book_support()` directly, same as the CLI does.

**One real gap this surfaces**: `ProjectPipeline::new` takes a `Format` value, not a
format string. The CLI builds it via `resolve_format()` (`render.rs:1825`), which is
`pub(crate)` to the `quarto` crate only — unreachable from `quarto-test`. Use
`Format::from_format_string` instead (`crates/quarto-core/src/format.rs:1125`,
`pub fn`) — does the same job, already exported for exactly this kind of cross-crate
use. Add this to the checklist explicitly so it isn't rediscovered mid-implementation.

## Checklist

- [ ] In `smoke_all.rs` (or a new project-aware sibling module — if a new file, it
      must follow `.claude/rules/integration-tests.md`'s pattern: register it via
      `pub mod <name>;` in `tests/integration/main.rs`, not a new top-level
      `tests/<name>.rs`), detect a `_quarto.yml` ancestor for each discovered `.qmd`
      via `ProjectContext::discover` (`crates/quarto-core/src/project/mod.rs:1887`,
      already reliably resolves `.dir` to project root from any file inside the
      tree); group files by project root. A per-file `ProjectContext::discover` call
      happens twice per file this way (once for grouping, once inside the render
      call below) — a minor inefficiency, not a correctness issue, worth a comment
      rather than premature optimization.
- [ ] Add a dedup cache (per test run) keyed by project root: the first file in a
      group whose front matter sets `render-project: true` triggers one whole-book
      render for that project; siblings in the same group reuse the result. No
      concurrency concerns — `smoke_all()` is a single `#[test] fn` iterating every
      fixture in one process (`smoke_all.rs:22`), not parallel workers.
- [ ] Add `render_project_document()` to `crates/quarto-test/src/runner.rs`, calling
      `ProjectPipeline::new(...).run_with_book_support()` — see "The entry point"
      above; both are already `pub`, no visibility changes needed. Build the
      `Format` value via `Format::from_format_string` (`format.rs:1125`, `pub`), not
      the CLI-only `resolve_format()` wrapper.
- [ ] Confirm `_quarto.tests.run.skip` (chapters/appendices/references) composes
      correctly: those files should be discovered (so their project is registered for
      the dedup cache) but never independently assert-checked.
- [ ] Zero hits today for `render-project`/`render_project` anywhere in `crates/`
      outside test fixtures — confirmed by grep. Q2's core render path has no
      awareness of this metadata key; this phase owns 100% of its interpretation,
      not a thin wrapper over existing logic.
- [ ] `cargo clippy -p quarto-test -p quarto --all-targets -- -D warnings` +
      `cargo nextest run -p quarto-test -p quarto`.

## Status

Not started.
