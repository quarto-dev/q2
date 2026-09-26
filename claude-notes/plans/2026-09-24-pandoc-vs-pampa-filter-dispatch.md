# Plan: Route `Position::Post` extension filters to pandoc's `main.lua` chain for Pandoc-hybrid targets (book-projects P2b)

**Date:** 2026-09-24
**Epic:** [`2026-09-21-book-projects-epic.md`](2026-09-21-book-projects-epic.md) (new phase, inserted between P2 and P3 — see the epic's phase table)
**Depends on:** P2's item 80 (`orange-book` vendored as a real extension subtree) — this is what first exercised the gap.
**Blocks:** P2's items 55–61 (the `orange-book`-grounded integration tests, currently unblocked-but-red), and P3 (wiring the merge output through real Typst/EPUB compilation) for any book that uses an extension filter with an `at:` entry point.

## Overview

Vendoring `orange-book` for real (P2 item 80) exposed a genuine, previously-latent
architecture gap: **every extension-contributed Lua filter runs through pampa's
native Lua engine (`UserFiltersStage`), regardless of what entry point the
extension's own `_extension.yml` declares.**

`orange-book.lua` declares `at: post-quarto` — one of Q1's five entry-point
names (`pre-ast`, `post-ast`, `pre-quarto`, `post-quarto`, `pre-render`).
These names only have real semantic meaning **inside `main.lua`'s own filter
list**: `main.lua` seeds a same-named slot in `quarto_filter_list` for each one
(`post-quarto`'s slot is pre-seeded with `file_metadata()` itself — see
`resources/pandoc-filters/filters/main.lua` around the `quarto_filter_list`
construction), and `inject_user_filters_at_entry_points`
(`resources/pandoc-filters/filters/ast/emulatedfilter.lua`) splices a
matching user filter in right there, so it shares `main.lua`'s Lua VM and
global state — including the pure-Lua helpers Q1 ports
(`quarto.doc.file_metadata`, `quarto.utils.combineFilters`,
`quarto.utils.file_metadata_filter`, all defined in
`resources/pandoc-filters/filters/common/{filemetadata,pandoc}.lua`).

Pampa's Lua engine (`crates/pampa/src/lua/`) is a **separate, hand-written
Rust reimplementation** of a subset of the Lua API
(`register_quarto_api`/`register_quarto_doc` in `quarto_api.rs`/
`quarto_doc.rs`). It has never loaded these Q1-ported pure-Lua helpers, so
any extension filter that relies on them — not just `orange-book`, any
extension using the file-metadata-marker convention or `combineFilters` —
crashes today with `attempt to call a nil value (field
'file_metadata_filter')` (or similar) when Q2 routes it through pampa.
Currently, `filter_resolve.rs`'s `resolve_filters` maps every `at:` value to
a `Position::Pre`/`Position::Post` bucket for `UserFiltersStage`, and that's
the *only* thing an `at:` entry point currently does — it never reaches
`main.lua`'s real entry-point mechanism at all.

**Two experiments already prove the fix direction** (both green,
`crates/quarto-core/tests/integration/orange_book_lua.rs`):

1. `orange_book_lua_loads_without_crashing_the_filter_chain` — loading the
   real, unpatched `orange-book.lua` through `main.lua`'s own `post-quarto`
   entry point (`run_main_lua_capturing_ast` with a `quarto-filters.entryPoints`
   param naming the real vendored file) does not crash.
2. `orange_book_lua_transforms_a_real_part_divider_via_pandocs_main_lua_chain`
   — with realistic content (a real merge-step-shaped
   `<!-- quarto-file-metadata: ... -->` marker + a `.quarto-book-part`
   heading), the same route correctly produces `#part[Part One]` Typst
   markup.

So the fix is **dispatch**, not new pampa API surface — and the dispatch
boundary is already implicit in Q2's own pipeline shape, not something to
invent. **Resolved with Gordon (2026-09-24): the split is exactly
`FilterPosition::Pre` vs. `FilterPosition::Post`, not "any `at:`-qualified
filter."**

`filter_resolve.rs`'s existing `ENTRY_POINTS` table already maps Q1's five
names onto Q2's two buckets:

```rust
("pre-ast", Position::Pre),
("post-ast", Position::Pre),
("pre-quarto", Position::Pre),
("post-quarto", Position::Post),
("pre-render", Position::Post),
```

`UserFiltersStage::pre()` runs *before* `AstTransformsStage` — i.e., before
Pandoc has any role in the render at all, so `Position::Pre` filters
(`pre-ast`/`post-ast`/`pre-quarto`) correctly stay on pampa's native engine
exactly as today; there is no pandoc process yet for them to join.
`UserFiltersStage::post()` runs *after* `AstTransformsStage`, which for a
Pandoc-hybrid target is at or past the point Q2's pipeline hands off to the
real `pandoc` subprocess (`PandocWriteStage`, which is what actually invokes
`-L main.lua`) — so `Position::Post` filters (`post-quarto`/`pre-render`)
are exactly the ones that should be redirected into `main.lua`'s own
entry-point mechanism instead of pampa, for Pandoc-hybrid-profile targets.
This is a clean, already-existing boundary — no new three-way classification
needed, just a different destination for filters already in the `Post`
bucket when the target is Pandoc-hybrid.

The redirect: for a Pandoc-hybrid-profile render, `Position::Post` filters
get forwarded into `main.lua`'s `quarto-filters.entryPoints` param (via
`PandocWriteStage`, which already has a working precedent for this shape —
`BookSingleFileContributor`, registered conditionally in
`PandocWriteStage::run()` when `single-file-book` is set) instead of being
run by `UserFiltersStage::post()`/pampa. `Position::Pre` filters are
untouched by this plan.

## Open questions to resolve during implementation (TDD, not upfront design)

- **Double-run avoidance.** `UserFiltersStage::post()` must not *also* run a
  filter that's been forwarded to `main.lua` for a Pandoc-hybrid target.
  Likely: `UserFiltersStage::post()` skips forwarding entirely (returns its
  input AST unchanged past the resolve step) when the pipeline profile is
  Pandoc-hybrid, and a new `PandocWriteStage`-side contributor reads
  `resolved.post` itself to build `quarto-filters.entryPoints`.
- **HTML is unaffected.** Confirm `Position::Post` filters for
  non-Pandoc-hybrid (native HTML) targets are untouched — there is no
  `main.lua` leg for HTML at all, so `UserFiltersStage::post()` must keep
  running them via pampa exactly as today. This is the negative control.
- **Which entry points actually round-trip.** `post-quarto` is proven
  (orange-book's own declared entry point). Confirm `pre-render` also
  round-trips correctly through `inject_user_filters_at_entry_points`
  before assuming the fix generalizes across the whole `Post` bucket, not
  just the one name tested so far.
- **Path resolution.** The forwarded filter needs an absolute path
  `main.lua` can `dofile`. Confirm this resolves correctly through the
  extension-subtree embed's lazily-extracted temp directory
  (`ORANGE_BOOK_SUBTREE.path()`), not just a source-tree path (which is all
  the current experiment tests exercise).

## Checklist

### Tests first
- [x] Two experiment tests already prove the fix direction — keep them as
      permanent regression guards for `main.lua`'s entry-point mechanism:
      `orange_book_lua_loads_without_crashing_the_filter_chain`,
      `orange_book_lua_transforms_a_real_part_divider_via_pandocs_main_lua_chain`
      (`crates/quarto-core/tests/integration/orange_book_lua.rs`).
- [x] Integration test: `pre-render` (the other `Position::Post` name) also
      round-trips correctly through `main.lua`'s real chain — extended
      `orange_book_lua.rs`'s pattern with a small synthetic fixture, not
      orange-book-specific: `pre_render_entry_point_round_trips_through_pandocs_main_lua_chain`.
- [x] Unit test: for a Pandoc-hybrid-profile render, the new mechanism reads
      exactly `resolved.post` (the existing `Position::Post` bucket) to
      build `quarto-filters.entryPoints` — no new classification logic,
      just a new consumer of the existing bucket.
      `test_quarto_filter_entry_points_contributor_overrides_empty_default`
      / `test_quarto_filter_entry_points_empty_without_contributor`
      (`pandoc_filters/params.rs`).
- [x] Regression test / negative control: for a **non**-Pandoc-hybrid (HTML)
      target, `Position::Post` filters continue to run via
      `UserFiltersStage::post()`/pampa exactly as today — proves the
      redirect is Pandoc-hybrid-only, not a global behavior change.
      `post_stage_html_target_still_runs_filters_via_pampa`, proven via a
      `SpyRuntime` that records `file_read` calls (a plain `Result::is_err()`
      check can't distinguish "ran and succeeded" from "skipped", since the
      existing `MockRuntime` returns `Ok(vec![])` for any path).
- [x] Regression test / negative control: `Position::Pre` filters
      (`pre-ast`/`post-ast`/`pre-quarto`) are untouched by this fix for
      *any* target, Pandoc-hybrid or not — they keep running via pampa.
      `pre_stage_runs_via_pampa_regardless_of_pandoc_hybrid_target`.
- [x] The currently-red end-to-end tests become the top-level regression
      guard once this lands — do not write new equivalents, just confirm
      GREEN: `book_single_file_merge::execution_skipped_is_true_when_a_chapter_engine_cell_is_excluded`,
      `book_single_file_merge::execution_skipped_is_false_when_no_chapter_has_an_engine_cell`,
      `book_single_file_merge::typst_book_merges_all_chapters_into_one_compiled_pdf`.
      Confirmed GREEN (all 3).

### Implementation
- [x] Land the redirect: `UserFiltersStage::post()` skips running
      `resolved.post` via pampa when the pipeline profile is Pandoc-hybrid
      (returns the AST untouched), and a new `PandocWriteStage`-side
      contributor (`QuartoFilterEntryPointsContributor`, mirroring
      `BookSingleFileContributor`) re-resolves `resolved.post` from
      `doc.ast.meta["filters"]` (left untouched by the skip) and forwards it
      into `quarto-filters.entryPoints`. Touched exactly the four files
      anticipated: `filter_resolve.rs` (added `ResolvedFilters::post_entry_points`,
      a parallel `Vec<&'static str>` carrying each post filter's specific Q1
      entry-point name — the Post bucket bundles 5 distinct names, so the
      bucket alone isn't enough to rebuild `at:`), `user_filters.rs`,
      `pandoc_filters/params.rs` (`EntryPointFilter`/
      `QuartoFilterEntryPointsContributor`), `stage/stages/pandoc_write.rs`.
- [x] Confirm path resolution against the real embedded/extracted
      `ORANGE_BOOK_SUBTREE` payload, not just a source-tree fixture path —
      confirmed by `book_single_file_merge.rs`'s 3 tests: the plain 2-chapter
      fixture has no explicit `_extension.yml`/`filters:` at all, so its
      green pass exercises the auto-default-to-`orange-book` path
      (`TYPST_BOOK_DEFAULT_EXTENSION`) through the real extracted subtree
      path end to end.
- [x] Update `resources/pandoc-filters/README.md` if the fix introduces any
      new tracked convention — **not needed**: the fix is entirely on Q2's
      Rust dispatch side (the four files above); no file under
      `resources/pandoc-filters/filters/` was touched, so there is no new
      vendored-tree convention to document.
- [x] Re-run and confirm GREEN: `book_single_file_merge.rs` (all tests),
      `orange_book_lua.rs`, plus the full `pandoc_*`/`user_filters*`/
      `crossref_numbering*` suites for regressions. 269 tests total
      (`pandoc*` + `user_filters*` + `crossref_numbering*` + `filter_resolve*`
      = 266, plus `book_single_file_merge`'s 3 run separately), all green.
- [x] `cargo clippy -p quarto-core --all-targets -- -D warnings` — clean.
      `cargo nextest run -p quarto-core --no-fail-fast`: **4976 passed (1
      slow), 31 skipped, 0 failed** — delta from the pre-fix baseline (4970
      tests, 3 failing, recorded above) is exactly `+6` new tests (3 in
      `user_filters.rs`, 2 in `params.rs`, 1 in `orange_book_lua.rs`) and the
      3 previously-red tests now passing; `git diff --stat` confirms every
      changed file in this phase is under `crates/quarto-core/`, so no other
      crate's test count could have moved. Phase-boundary
      `cargo nextest run --workspace`: **14828 passed (1 slow), 200 skipped,
      0 failed** — fully accounted for by the same quarto-core-only delta
      above (attributed from full-phase context per the workspace-run rule,
      not re-derived by bisecting).

## Details

**Why this isn't itemized in P2's own plan.** P2's plan assumed (Decision
26/Decision "emit both markers") that emitting the comment markers plus a
small tracked patch to `book-numbering.lua` would be sufficient for an
*unpatched* extension like `orange-book` to work. That assumption was correct
about *what data* the extension needs; it was silent on *which Lua engine*
runs the extension's filter at all — a question that only became answerable
once a real, non-fixture extension was vendored (item 80) and actually
exercised end-to-end. This is exactly the kind of "would have caught it
earlier" gap P2's own item 55 anticipated in the abstract; this plan is the
concrete fix that item 55's test needs to pass.

**Why this affects more than book-projects.** Any Q2 extension — not just
Typst book extensions — that declares a `Position::Post` entry point
(`post-quarto` or `pre-render`) on a Pandoc-hybrid target and relies on
Q1's file-metadata-marker convention, or any other `main.lua`-only pure-Lua
helper, is silently broken today wherever Q2 resolves it as a "user filter."
This plan's fix should be written generically (dispatch by `Position::Post`
+ Pandoc-hybrid pipeline profile), not special-cased to `orange-book` or to
book projects, even though book-projects P2 is the first real consumer and
regression guard.
