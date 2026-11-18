---
title: 'Fold `header-includes` / `include-before` / `include-after` after user filters (GH #1257)'
date: 2026-11-18
description: 'A Lua filter that sets `header-includes`, `include-before`, or `include-after` has no effect on HTML output. The inline keys are folded into `rendered.includes.*` before filters run. This plan folds them after filters, as Pandoc and Quarto 1 do.'
status: done  # Implemented 2026-11-18; PR pending review.
---

**Branch:** `issue-1257` (worktree `workspace-4`), from `origin/main` \@ `c7eb54559`.
**GitHub issue:** [#1257](https://github.com/quarto-dev/q2/issues/1257).
**Related:** bd-7avt1ogu (the same keys are dropped for Pandoc-hybrid formats; separate path).

## Overview

In Quarto 1, a Lua filter can set the inline include keys from `Meta`, and the content reaches
the output. In q2 the filter's value is ignored.

### Why q2 differs (a regression, not an original gap)

- **Pandoc.** The writer's template context is `variables <> metadata`
  (`external-sources/pandoc/src/Text/Pandoc/Writers/Shared.hs:130`, `addVariablesToContext`).
  `-H`, `-B`, and `-A` files are *variables* (`App/OutputSettings.hs:210-214`); filters never see
  them. The metadata is the final, post-filter metadata. For the same key the two concatenate:
  files first, then the metadata value.
- **Quarto 1** turns authored `include-in-header` etc. into `-H` files and passes front-matter
  `header-includes` through as metadata. Q1's own Lua helpers (`common/meta.lua`, `addInclude`)
  also append to the metadata key.
- **q2 before 2026-05-04.** `set_includes_list` read the metadata keys at template time, after
  filters, so filter writes worked.
- **`2026-05-04-includes-feature.md`** moved the read into `IncludeResolveStage`, before the
  profile checkpoint, and made the template read only `rendered.includes.*`. That plan described
  Q1's flow only up to "Pandoc reads the file from disk" and treated filters as readers of
  `rendered.includes.*`, never as writers of the top-level keys. It also moved the generate step
  from the planned `AstTransforms` Normalization phase to a pre-checkpoint stage without
  discussing filters.

### Design

Keep the early fold for what needs it (file slots, which feed `profile.includes`). Stop folding
the three inline keys early. Fold the *final* values of the inline keys in a new stage placed
immediately after `UserFiltersStage::post()`, so every downstream consumer of
`rendered.includes.*` (the HTML template, the reveal scaffold, the preview head injector) sees
them.

- The final metadata is authoritative, so a filter can append to, replace, or clear an authored
  value.
- No double counting: the early fold no longer reads the inline keys.
- Ordering follows Q1/Pandoc: for each slot, the file-slot entries come first, then the inline
  entries. This changes the current order (inline before file-slot) for documents that set both.
  Engine contributions keep their current positions.
- The template keeps excluding the three keys from the context, and keeps reading
  `rendered.includes.*`.

## Checklist

### Phase 1: tests first

- [x] Plan file written.
- [x] Integration test through `render_document_to_file`: a filter that sets `header-includes`
  from `Meta` reaches `<head>`. Verify it fails first.
- [x] Integration test: a filter appends to a front-matter `header-includes`; both entries appear.
- [x] Integration test: the same for `include-before` and `include-after`.
- [x] Integration test: a filter that clears an authored `header-includes` removes it.
- [x] Unit tests: `resolve_includes` no longer folds the inline keys; the new stage folds them
  after the file-slot entries.

### Phase 2: implementation

- [x] Remove the inline-key reads from `resolve_includes`; update its ordering doc comment.
- [x] Add `IncludeInlineFoldStage` (`include_resolve.rs`), register it in `stage/stages/mod.rs`.
- [x] Insert it after `UserFiltersStage::post()` in `build_html_pipeline_stages_with_options`.
- [x] Check the pandoc-hybrid, q2-preview, pause, and finishing stage lists still validate.
- [x] Fix the stale comments in `pipeline.rs` and `template.rs`.

### Phase 3: verification

- [x] `cargo clippy -p quarto-core --all-targets -- -D warnings` and
  `cargo nextest run -p quarto-core`.
- [x] End to end: `cargo run --bin q2 -- render` of the issue's `og.lua` repro; inspect `<head>`.
- [ ] `cargo xtask lint` (part of `cargo xtask verify`, running); smoke-all fixtures that set both inline and file-slot includes.
- [ ] Recommend a downstream-dependents run before the push (no push without approval).

## Outcome

- Added `IncludeInlineFoldStage` (after `user-filters-post`); `resolve_includes` no longer reads the
  inline keys. The pandoc-hybrid pipelines keep the stage, which bd-7avt1ogu can reuse.
- Tests: seven new (five HTML, one reveal deck, one stage-order guard across the HTML and
  q2-preview pipelines). The reveal and stage-order tests were written after the fix and did not fail
  first. Two `include_resolve` unit tests and four `pipeline.rs` stage-list assertions changed to
  match the new order and the extra stage.
- End to end: `cargo run --bin q2 -- render test.qmd` with the issue's `og.lua` now writes
  `<meta property="og:title" content="test">` into `<head>` (inspected).
- Not covered: a render of the preview head injector, and a filter in the pre position.
