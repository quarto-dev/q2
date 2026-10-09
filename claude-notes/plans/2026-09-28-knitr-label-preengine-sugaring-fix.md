---
title: 'Fix: restore knitr label visibility through PreEngineSugaringStage'
date: 2026-09-28
description: 'Restores label-derived knitr figure filenames, such as `fig-cars-1.svg`, by re-injecting the `label` option into the engine input that pre-engine sugaring had removed.'
---

**Strand:** bd-2lxj10z0 — knitr label stripped by PreEngineSugaring breaks
label-based figure filenames (`fig-cars-1.svg` -> `unnamed-chunk-1-1.svg`)
**Public tracking issue:** https://github.com/quarto-dev/q2/issues/741
**Worktree:** `.worktrees/workspace-4`, branch
`braid/bd-2lxj10z0-knitr-label-stripped-preenginesugaring`, forked from
`typst-testing/p8-orange-book-base` at `5b0dc2528`.

## Problem, in one paragraph

`PreEngineSugaringStage` (`crates/quarto-core/src/crossref/codeblock_shorthand.rs`)
lifts a crossref-classified `label:` (e.g. `fig-cars`) out of the code cell body
and into the wrapping `::: {#fig-cars}` Div, for q2's own crossref numbering.
`EngineExecutionStage::serialize_ast_to_qmd` then serializes that already-desugared
AST and hands it to knitr — which never sees `label:` at all, so it falls back to
its generic positional name (`unnamed-chunk-N`) instead of the label-derived one
(`fig-cars-1.svg`) Quarto 1.x and bare knitr both produce. This breaks any
reference (in this project or another file) to the artifact by its expected name.

## Decisions

**D1 — Fix knitr only, not Jupyter, in this pass.**
q2's Jupyter engine (`crates/quarto-core/src/engine/jupyter/text_execute.rs`) does
not implement label-based figure naming at all yet — it names outputs purely
positionally (`cell-<i>-output-<j>.<ext>`, see tests at lines 1658/1681/1717/1772).
Jupyter parity is a separate, not-yet-built feature; when it's built, it should be
built label-aware from the start (Q1's own convention: `<label>-output-<n>.<ext>`,
see `external-sources/quarto-cli/src/core/jupyter/{labels.ts,jupyter.ts}`), so it
won't need "unstripping" the way knitr does. Do not expand this strand's scope to
cover Jupyter.

Follow-up filed 2026-09-28: `bd-6sb1z0i4` ("Extend knitr label-based output
naming to Jupyter, then drop the `engine.name() == \"knitr\"` gate"),
`discovered-from` this strand — Jupyter-side label-aware output naming, and
removing the knitr-only gate in `engine_execution.rs` once it lands.

**D2 — Fix is additive: inject at serialization, not in the AST.**
Re-emit `#| label: <id>` into the *text sent to the engine* inside
`serialize_ast_to_qmd` (`crates/quarto-core/src/stage/stages/engine_execution.rs:867`),
without touching the stored pre-engine `CodeBlock.text` and without removing the
wrapping Div's `id` attribute. The Div still needs the label independently for
crossref numbering — that consumption path in `codeblock_shorthand.rs` does not
change. Putting the label back into the AST's `cb.text` instead would reintroduce
the exact duplication `PreEngineSugaringStage`/D2 (`claude-notes/plans/2026-04-15-crossref-design.md`)
was built to remove.

**D3 — Reconciliation-equality risk is real but likely already covered; verify, don't assume.**
`quarto_ast_reconcile::reconcile(ast, executed_ast)` (called at
`engine_execution.rs:815`) uses a structural-equality check
(`crates/quarto-ast-reconcile/src/hash.rs:830`:
`(Block::CodeBlock(a), Block::CodeBlock(b)) => attr_eq(&a.attr, &b.attr) && a.text == b.text`)
to decide whether a code block is unchanged across the pre/post-engine round-trip.
If the injected `#| label:` line leaks into the *echoed* code that comes back from
knitr, the reparsed post-engine `CodeBlock.text` would differ from the stored
pre-engine one and this equality would break.

Mitigating evidence: `crates/quarto-core/src/engine/knitr/resources/rmd/hooks.R`
is a near-verbatim copy of Q1's `src/resources/rmd/hooks.R` (3 incidental line
diffs only — `diff` them to confirm if in doubt) and already contains an
echo-filter list, `quarto_opts` (hooks.R lines ~348-361), that explicitly includes
`"label"`. This is Q1's own native mechanism for hiding label/cap/etc. from the
*displayed* source code while still using them internally — untouched, so it
should apply here too once label reaches knitr's real option parser. This is an
expectation, not a verified fact: **write a test that renders a labelled,
`echo: true` chunk through the real pipeline and asserts the rendered source-code
block does not literally show `#| label: fig-cars`,** rather than trusting this
reasoning alone.

**D2/D3 status (2026-09-28, implemented and verified this session).**
Implemented as `crates/quarto-core/src/crossref/label_reinject.rs::inject`,
called from `EngineExecutionStage::run` right after `nested_cell_mask::mask`
on the per-engine `masked_ast` clone, gated `if engine.name() == "knitr"`
(D1). It walks the same containers `codeblock_shorthand::desugar_blocks`
recurses into, finds a Div whose `id` classifies via `ctx.ref_type_registry`
(skips cleanly if `None`, e.g. replay paths that never ran
`PreEngineSugaringStage`), and prepends `#| label: <id>` (cell's own
comment syntax) to the wrapped CodeBlock's cloned text. D3's empirical test
is `crates/quarto-core/tests/integration/knitr_label_reinject.rs`
(real knitr, no mocks): `labelled_figure_uses_label_derived_filename`
confirms the label reaches knitr's filename logic (verified RED without
the fix — `unnamed-chunk-1-1.png` — GREEN with it — `fig-cars-1.png`), and
`label_option_is_not_echoed_in_rendered_source` confirms hooks.R's
`quarto_opts` filter does hide it from `echo: true` output, exactly as D3
hypothesized.

**D4 — Census of what else is consumed: nothing else needs this fix.**
`codeblock_shorthand.rs`\'s consumed set is `label` (crossref-classified only),
`<reftype>-cap` (registry-driven — `fig-cap`/`tbl-cap`/`lst-cap`/etc.), `fig-scap`
(only on the *unlabelled* Figure path), and `fig-alt` (only on diagram/mermaid
cells). Checked each against knitr's native semantics
(`~/src/quarto-web/docs/reference/cells/cells-knitr.json` + hooks.R):
- `*-cap` options only affect caption *text* — q2's own crossref rendering
  supplies the caption anyway (hooks.R's native plot/figure hooks are fully
  overridden), so consuming them has no engine-visible side effect. Not a bug.
- `fig-scap` on a *labelled* figure is not consumed at all (only the unlabelled
  path consumes it) — already reaches the engine untouched.
- `fig-alt` is only consumed for diagram (mermaid) cells — real knitr/Jupyter code
  cells never have it stripped.
Do not widen this fix to re-inject anything beyond `label`.

**D5 — Related, out-of-scope side effect worth a one-line test/comment, not a fix.**
knitr's own `cache`/`dependson` chunk options key their on-disk cache files by the
chunk's real (knitr-internal) label. Since a labelled, cached chunk currently gets
the positional `unnamed-chunk-N` pseudo-label instead, its cache key is
position-dependent rather than label-dependent — inserting/removing an earlier
chunk could silently invalidate or cross-wire caches. This should self-resolve
once D2 lands (knitr will see the real label again); no separate fix needed, but
worth a regression test alongside the filename one if convenient.

**D5 status (2026-09-28): added, confirmed self-resolved.**
`labelled_cached_chunk_uses_label_derived_cache_key` in
`knitr_label_reinject.rs` renders a `#| label: fig-cars` + `#| cache: true`
chunk and asserts the `<stem>_cache/html/` entry is named `fig-cars_*`, not
`unnamed-chunk*`. Passes with the fix in place, as predicted — no separate
fix was needed.

**D6 — Orange-book fixture workaround should be reverted once the fix lands.**
`crates/quarto/tests/smoke-all/typst/orange-book/{chapter2,appendix,appendix-b}.qmd`
currently hardcode `chapter1_files/figure-typst/unnamed-chunk-1-1.svg` (6
occurrences) — a deliberate workaround Gordon applied to unblock P8 without
fixing this bug. Q1's originals (`external-sources/quarto-cli/tests/docs/smoke-all/typst/orange-book/`)
use `fig-cars-1.svg`. Once the fix produces `fig-cars-1.svg` again, revert these
6 references to match Q1, and confirm the smoke-all fixture still passes.

**D6 status (2026-09-28, verified this session): already reverted upstream —
nothing left to revert.** Commit `7263b9453` ("P8: reconcile orange-book
fixture to Q1 source...", already an ancestor of this branch's `5b0dc2528`
base) flipped all 6 references to `fig-cars-1.svg` before this strand
started. Confirmed empirically with `SMOKE_FILTER=orange-book cargo test -p
quarto --test integration -- smoke_all`, comparing cold-cache runs with the
fix stashed vs. applied:
- **Without the fix:** `typst/orange-book/index.qmd` fails with exactly
  bd-2lxj10z0's signature — `error: file not found (searched at
  .../chapter1_files/figure-typst/fig-cars-1.svg)` — because the real
  render still produces `unnamed-chunk-1-1.svg` and the (already-reconciled)
  fixture expects `fig-cars-1.svg`.
- **With the fix:** that specific file-not-found error is gone (the SVG is
  now produced and found under the expected name). The book render still
  fails, but on unrelated, pre-existing problems downstream of the fixed
  path — a `label <fig-cars> occurs multiple times` error and `the
  document does not contain a bibliography` errors. Both are consistent
  with CLAUDE.local.md's note that a sibling worktree (`.worktrees/workspace-5`,
  same `5b0dc2528` base) has uncommitted, in-progress P8 work touching
  `citeproc_filter.rs` and orange-book `.qmd` files — i.e. the bibliography
  wiring for this exact book is being fixed elsewhere, concurrently. Out of
  scope for bd-2lxj10z0 (D1); do not fix here.
- `typst/orange-book-lang/index.qmd` and `typst/override-orange-book/index.qmd`
  also fail, identically, with or without this fix (`Error attempting to
  write crossref index`, `crossref/index.lua:215`) — both fixtures have
  **zero** code cells, so knitr/`label_reinject` cannot be involved; this is
  a pre-existing, unrelated flake (looks like a cold-cache directory-creation
  race for the xref index write) and is also out of scope here.

## Verification gates (per user's global testing rule — this overrides any
"workspace run every fix" default)

- `cargo clippy -p quarto-core --all-targets -- -D warnings` +
  `cargo nextest run -p quarto-core` after the code change.
- A full `cargo nextest run --workspace` at the phase boundary (before merge/push),
  reported as a delta against the live baseline at this branch's HEAD — not a
  number copied from an older doc.

**Gate results (2026-09-28, this session, all against branch HEAD `5b0dc2528` +
this session's uncommitted changes):**

- `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean.
- `cargo nextest run -p quarto-core`: **5294 tests run, 5294 passed, 32
  skipped.** Delta against this session's own pre-change baseline run
  (5291/5291 passed, 32 skipped): **+3, all new** — exactly the 3 tests
  added in `knitr_label_reinject.rs`. No stray or duplicated tests.
- `cargo nextest run --workspace --no-fail-fast`: **15261 tests run, 15259
  passed, 2 failed, 201 skipped.** Both failures verified (via stash/apply
  A-B, fix removed vs. restored, same command) to be **pre-existing,
  unrelated to this change**:
  - `quarto::integration smoke_all::smoke_all` — fails identically with or
    without the fix; see D6 status above (pre-existing orange-book
    bibliography/citeproc gap being worked in `.worktrees/workspace-5`,
    plus an unrelated cold-cache crossref-index-write flake in
    `orange-book-lang`/`override-orange-book`, both fixtures having zero
    code cells).
  - `quarto-test runner::tests::should_error_respects_project_render_context`
    — fails identically with or without the fix. Its fixture chapter has no
    `label:` option at all, so `codeblock_shorthand`\'s desugar never
    produces a wrapper Div for it and `label_reinject::inject` is
    necessarily a no-op on this AST; not investigated further as it is out
    of this strand's scope (D1), but ruled out as a regression.
  - No other workspace crate shows a delta from this change.
