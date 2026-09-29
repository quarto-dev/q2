# P8 — Port `orange-book` (base), full predicates

**Date:** 2026-09-27
**Epic:** [`2026-09-27-typst-smoke-all-epic.md`](2026-09-27-typst-smoke-all-epic.md) —
Decided item 5: full predicate scope, not the reduced "one assertion" scope floated
during research.
**Depends on:** P1, P3, P6, P7.
**Worktree:** `workspace-5` (Track B, sequential after P7 — see epic's "Parallel
development plan"). **This is the one cross-track blocking point in the whole
epic**: P3 is done in `workspace-2` (Track A), not this worktree — don't just check
P7, check P3 too.

## Worktree & git workflow

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3
git show feature/typst-testing:claude-notes/plans/2026-09-27-typst-smoke-all-epic-P7-override-and-lang.md \
  | grep -q '^Complete\.' && echo "P7 merged" || echo "STOP: P7 not yet merged, wait"
git show feature/typst-testing:claude-notes/plans/2026-09-27-typst-smoke-all-epic-P3-struct-tree-walk.md \
  | grep -q '^Complete\.' && echo "P3 merged" || echo "STOP: P3 not yet merged — this is the cross-track wait, check back later, don't start P8 without it"
```

Only proceed once **both** print "merged". If P3 isn't merged yet, there is no more
Track-B-only work to do before P8 — stop here and re-run this check later (e.g. at
the start of a new session) rather than guessing at P3's interface.

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5
git checkout -B typst-testing/p8-orange-book-base feature/typst-testing
```

Implement the checklist below, gating on `cargo clippy -p quarto --all-targets --
-D warnings` + `cargo nextest run -p quarto`. When done, flip this doc's checklist
to `[x]` and `## Status` to `Complete.`, commit, then:

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5
git rebase feature/typst-testing
cd /Users/gordon/src/q2/.worktrees/workspace-3
git checkout feature/typst-testing
git merge --ff-only typst-testing/p8-orange-book-base   # retry from rebase (in workspace-5) if not a fast-forward
cargo nextest run --workspace                  # phase-boundary gate
```

`workspace-5` is now free — check whether `workspace-2` still needs help finishing
P5 (see P9's doc for the exact "is P5 merged yet" check) before picking up P9.

## Scope

`external-sources/quarto-cli/tests/docs/smoke-all/typst/orange-book/` — the deep
torture-test fixture. Full multi-file book project: `_quarto.yml`
(`project: type: book`, 2 parts/3 chapters + 2 appendices + references,
`bibliography: references.bib`, `citeproc: true`, custom crossref kind `dino`,
`format.typst.keep-typ/toc-depth/theorem-appearance`), `_brand.yml` (color + logo).

`index.qmd` carries all assertions (~270 lines of front matter):
`ensureTypstFileRegexMatches` (~140 patterns: labels, `#part[...]`, appendix
`#show: appendices.with(...)`, brand-color/logo injection, custom-crossref-kind
counters), `ensurePdfRegexMatches` (~110 patterns: cross-chapter numbering,
appendix-A/B independent lettering, citeproc author-date bibliography, sub-figure
panels, custom "dinosaur" crossref kind), and exactly one `ensurePdfTextPositions`
assertion (plain-string `leftAligned`, no `granularity`/`role` — the simplest possible
case, already proven by P4/P5 before this phase needs it). Chapters/appendices/
references carry `_quarto.tests.run.skip: "Book chapter - only renders as part of full
book"`.

This is where P6's `render-project: true` dedup and P1's PDF-regex assertions get
exercised at full depth for the first time, and where P3's struct-tree walk gets
proven inside a real book-merge render (as opposed to P5's simpler independent-file
renders).

**The custom crossref kind and brand injection are lower-risk than they look —
verified during plan review, not just plausible.** A custom `crossref.custom` kind
needs zero Lua-filter-level extension work: it's Q2's native mechanism
(`crates/quarto-core/src/crossref/metadata.rs`'s `read_custom`, registered via
`RefTypeRegistry::register_custom`, `registry.rs:198`) declared purely in
`_quarto.yml`, not in the vendored extension's own Lua. More importantly, **this
exact scenario is already proven through the book-merge path on `main` today**:
`crates/quarto-core/tests/integration/book_numbering_torture.rs` declares a
`crossref.custom` kind (`key: dino`, `reference-prefix: Dinosaur`) in its own test
project and asserts `"Dinosaur 1.1: This is the first dinosaur."` against a real
compiled, book-merged PDF (`:64-65,288`) — currently green. Treat dino-through-merge
as confirmed, not a risk to re-derive. Brand injection is similarly lower-risk:
`resolve_typst_brand_param` (`pandoc_write.rs:248`) recomputes the brand fresh from
the merged document's own metadata at the finishing-stage tail (not a stateful field
needing book-merge carry-forward the way `ref_type_registry` is), and the metadata
merge's retain predicate only strips `"format"` (`metadata_merge.rs:471`), so
`brand:` survives into the merged document the same way `crossref` does. The one
genuine gap: nobody has actually rendered a book fixture with `_brand.yml`
end-to-end yet — this phase is the first real spike for that, specifically for
logo-path resolution against a synthetic merged document's directory context.

## Checklist

- [x] Copy the fixture directory's **tracked source files** into
      `crates/quarto/tests/smoke-all/typst/orange-book/` (all `.qmd`/`.bib`/
      `_brand.yml`/`logo.svg`/`notebooks/` files) — not a literal directory copy.
      Q1's checkout carries generated/local cruft alongside the source (`.quarto/`
      caches, `_book/` pre-rendered output, `index.typ`, and in at least one sibling
      `orange-book*` fixture a stray `.claude/settings.local.json` accidentally
      committed into the `quarto-cli` checkout) — each fixture's own `.gitignore`
      lists what's generated; copy everything except those.

      **Reconciled 2026-09-28.** The fixture had already been copied in an earlier
      session but had drifted from Q1's tracked source with a mix of intentional
      and stray changes; diffed every file against
      `/Users/gordon/src/quarto-cli/tests/docs/smoke-all/typst/orange-book/`
      (Q1's tracked `git ls-files`, not a literal directory listing) to sort them
      out:
      - Removed generated cruft not in Q1's tracked source: `.quarto/render-manifest.json`
        (empty Q2 render-cache artifact), `chapter1_files/` (stale Q2 render output,
        matches Q1's own `*_files/` gitignore pattern — Q1's real `chapter1_files/`
        has different contents, `fig-cars-1.svg` + a notebook PNG, neither of which
        this stray copy had), `chapter2.qmd.bak`/`chapter3.qmd.bak` (byte-identical
        backups of the already-edited files, safe to drop).
      - Reverted `_quarto.yml`'s `citeproc: false` back to Q1's `citeproc: true` —
        this was a leftover from earlier debugging that silently routed around the
        BibTeX bibliography-parse failure (see item 4) rather than exposing it;
        the plan's own Scope section calls for `citeproc: true`.
      - **Did NOT revert** the escaped apostrophes in `chapter2.qmd`/`chapter3.qmd`
        (`@hoare1978's` → `@hoare1978\'s`, etc.) — initially mistook these for
        spike debris and reverted them too, which immediately reproduced a real,
        documented Q2 parser limitation: `[Q-2-7] Unclosed Single Quote` (an
        unescaped `'` right before Markdown syntax, e.g. before a citation's
        trailing text, is misparsed as an opening smart-quote with no close). This
        is a known, tooled port-time fix — see
        `crates/qmd-syntax-helper/src/conversions/q_2_7.rs` — not a workaround for
        anything BibTeX/citeproc-specific. Re-applied the escapes; this is now the
        one intentional content deviation from Q1's tracked source, required for
        this fixture to parse under Q2 at all.
      - `index.qmd`'s only diff from Q1 is the `requires: jupyter` block added in
        an earlier session (see item 3 — since superseded by a `run.skip`, see
        item 4).
- [x] Set `index.qmd`'s `render-project: true` (confirm it's already set in Q1's
      fixture; add if not, matching Q1's own convention). Already set; unchanged.
- [x] `chapter1.qmd` (carries `_quarto.tests.run.skip`) uses
      `{{< embed notebooks/computations.ipynb#fig-visualization >}}`, and neither it
      nor `index.qmd` declares `requires: jupyter`. Confirm whether the book-merge
      path needs a live Jupyter runtime to process this embed, or only reads
      pre-baked notebook outputs — if the former, this fixture needs the same
      `requires: jupyter` gate `crates/quarto-test/src/runner.rs:100-101,137-152`
      already provides for other fixtures, or it will hard-fail (not cleanly skip)
      on machines without Jupyter installed.

      **Resolved 2026-09-28: neither — `{{< embed >}}` is not implemented in Q2 at
      all, so jupyter-availability is moot.** `dispatch_shortcode` in
      `crates/quarto-core/src/transforms/shortcode_resolve.rs` only has three
      built-in Rust handlers (`meta`/`env`/`var`) and falls through to Lua
      handlers; there is no `embed.lua` anywhere in the tree and no Rust handler
      named `embed`. `claude-notes/plans/2026-07-31-shortcode-extensions-port.md`
      confirms this explicitly (D6, confirmed 2026-07-31): "`embed` is out of
      scope for this plan... needs a more [substantial] redesign... own epic,
      engine-dependent." An unresolved shortcode call produces a Q-16-3
      "Shortcode handler not found" warning + an inline `**?embed**` marker, not a
      hard failure — engine-resolution for chapter1.qmd is driven only by its own
      `” `{r}` code chunk (the `fig-cars` plot), which knitr/R (available on this
      dev machine) handles fine regardless. Verified empirically: with the
      `requires: jupyter` gate removed, `SMOKE_FILTER=typst/orange-book/index.qmd`
      got past R-engine execution (`processing file: chapter1.rmarkdown` succeeded)
      and ran until the real, pre-existing blocker — the BibTeX bibliography-parse
      failure (see item 4) — with no jupyter involved at any point. Removed the
      `requires: jupyter` gate as based on an incorrect premise.

      Net effect on scope: `embed`'s absence will cost a handful of assertions
      once item 4 unblocks — only 3 lines in `index.qmd` reference
      `fig-visualization`/`<sec-embedded-notebooks>` — not the whole fixture. Not
      a P8 task to fix (own epic, per D6); tracked here as an accepted, understood
      gap to revisit once that epic lands, not a new strand (per this branch's
      "P8-scoped fixes and findings stay in the P8 plan" constraint).
- [ ] Render the whole book via P6's harness; confirm all ~140
      `ensureTypstFileRegexMatches`, ~110 `ensurePdfRegexMatches`, and the one
      `ensurePdfTextPositions` assertion pass against Q2's real, unmodified vendored
      `orange-book` extension. Treat this as the first real end-to-end exercise of
      `_brand.yml` through the book-merge path (see note above) — a logo-path
      mismatch here is a genuine finding, not expected to be pre-ruled-out.

      **2026-09-28, BibTeX blocker (item as originally written): reproduced,
      then worked around temporarily (Gordon's call) to keep making progress
      without waiting for bd-l6eh1635.** Reproduced the exact baseline the
      handoff described: `citeproc failed for merged book: Failed to parse
      bibliography '.../references.bib': expected value at line 1 column 1` —
      `load_bibliography` in `crates/pampa/src/citeproc_filter.rs` only parses
      CSL-JSON today; `references.bib` is BibTeX and isn't valid JSON. The real
      fix (biblatex-based `.bib` parsing) is still bd-l6eh1635, in separate,
      active development in `workspace-6` (WIP, not yet committed/merged there
      either) — stays out of workspace-5 per the handoff's constraints.

      **Temporary workaround, explicitly reversible**: `references.bib` is
      kept in the tree untouched as the real source. Generated
      `references.json` alongside it via `pandoc -f biblatex -t csljson
      references.bib -o references.json` (21/21 entries carried over cleanly)
      and pointed `_quarto.yml`'s `bibliography:` at the `.json` file, with a
      comment marking this temporary and naming what to revert once
      bd-l6eh1635 merges (swap back to `references.bib`, delete
      `references.json`). This unblocks rendering *now* without depending on
      bd-l6eh1635's timeline, and without touching or duplicating any of its
      actual parser work.

      **This surfaced a second, real, separate bug — now fixed on this
      branch**: past the bibliography load, citeproc failed with `Citation
      processing error: Reference 'sec-basic-figures' not found` — exactly the
      failure the original handoff already flagged as a distinct concern ("Do
      not assume the BibTeX parser fix resolves this"). Root cause, confirmed
      by reading code and reproducing empirically: the single-document
      pipeline resolves `citeproc` into the `.post` filter bucket by default
      (`filter_resolve.rs`, `"citeproc" into .post`), so citeproc always runs
      *after* `AstTransformsStage`'s Crossref sub-phase (`crossref-index` +
      `crossref-resolve`) has already reclassified reserved-prefix keys
      (`@sec-...`, `@fig-...`, …) out of plain `Inline::Cite` nodes. The
      book-merge driver (`crates/quarto-core/src/project/book/single_file_render.rs`)
      didn't follow that convention: it called `apply_citeproc_filter` once,
      directly, on the merged document *before* the merged whole's own
      Crossref-onward pipeline ran — a deliberate P2 design choice
      (`claude-notes/plans/2026-09-21-book-projects-P2-single-file-merge.md`)
      whose own tests only ever exercised pure bibliographic citations, never
      citeproc mixed with numbered crossrefs in the same book. `embed`'s
      absence (item 3) is unrelated — this reproduces independent of it.

      **Fix (Gordon-approved, 2026-09-28)**: split the merged document's
      finishing pipeline into three steps instead of one — run
      `AstTransformsStage::for_range(Crossref..=Crossref)` alone first (via a
      new intermediate `run_pipeline_from_ast` call), call
      `apply_citeproc_filter` on the result, then run the remaining
      `Navigation..`-onward stages as before
      (`build_pandoc_pipeline_finishing_stages(TransformPhase::Navigation,
      ...)` instead of `..Crossref`). No new pipeline primitive needed —
      `AstTransformsStage::for_range` already accepts arbitrary phase ranges.
      Verified: the `sec-basic-figures` citeproc error is gone; render now
      gets past citeproc entirely. Regression-checked broadly, not just the
      6-test subset — `cargo nextest run -p quarto-core -E` against all 15
      book-*/orange_book_lua test files (63 tests) and the full `-p
      quarto-core` suite (5282 tests) both green, no regressions. `cargo
      clippy -p quarto-core --all-targets -- -D warnings` clean.

      **Phase-boundary `cargo nextest run --workspace`**: 14962 passed / 1
      failed / 201 skipped (15247 total, plus the smoke-all `-p quarto` run
      counted separately above). The one failure —
      `quarto-test runner::tests::should_error_respects_project_render_context`
      — is **pre-existing, not caused by this fix**: confirmed by reverting
      `single_file_render.rs` to `HEAD` and rerunning the test in isolation,
      which still fails the same way (`assertion failed: matches!(result,
      TestResult::Pass | TestResult::Skipped(_))`, an R/knitr execution
      inside the test's own synthetic fixture). Unrelated to P8/citeproc/
      crossref work; not investigated further here (out of this branch's
      scope), restored the fix immediately after confirming.

      **Third bug — found and fixed 2026-09-28.** Render failed with `Error
      [Q-20-3]: pandoc exited with exit status: 6 ... Argument of --toc-depth
      must be a number 1-6` — this fixture's `_quarto.yml` deliberately sets
      `format.typst.toc-depth: 17` (Typst's native outline isn't capped at 6;
      Q1's fixture exercises that). `crates/quarto-core/src/pandoc_filters/format_defaults.rs`
      unconditionally forwarded `toc-depth` metadata as a raw `--toc-depth
      <n>` pandoc CLI argument, and pandoc's CLI flag parser hard-validates
      that range (verified directly: `pandoc -f markdown -t typst
      --toc-depth=17` → same error; `pandoc --defaults=<yaml with toc-depth:
      17>` → succeeds, no validation at all).

      Root-cause investigation (self-directed, not delegated to a subagent
      per this branch's constraint) found the CLI args are simply dead
      weight for Typst: both Q2's own default template
      (`resources/pandoc-filters/typst-template/typst-show.typ:89-99` →
      `typst-template.typ:122-134`) and every vendored extension's own
      template (orange-book's `typst-show.typ:24-25`) read `toc`/`toc-depth`
      purely as `$toc$`/`$toc-depth$` *template variables* — which pandoc
      always populates straight from the document's own metadata regardless
      of CLI flags — feeding Typst's own native `#outline(depth: toc_depth)`
      call directly. Pandoc's internal `--toc`/`--toc-depth`-driven TOC
      insertion is never consumed by the Typst writer in this codebase.
      Confirmed via LaTeX/PDF being a genuinely different (non-Typst) pandoc
      profile that does rely on these flags, so the fix must not touch it.

      **Fix (Gordon-approved, 2026-09-28)**: skip forwarding `--toc`/
      `--toc-depth` when `base_format == FormatIdentifier::Typst`, matching
      this same file's existing format-conditional-exception pattern
      (`template` is already skipped for Typst; `slide-level` is already
      Pptx-only). Added `test_toc_args_not_forwarded_for_typst` (RED before
      the fix — both args forwarded, `17` included verbatim) and
      `test_toc_args_still_forwarded_for_non_typst` (negative control,
      Docx). `cargo nextest run -p quarto-core -E 'test(format_defaults)'`:
      19/19 passed. `cargo clippy -p quarto-core --all-targets -- -D
      warnings` clean.

      **Fourth bug — found 2026-09-28, not yet fixed, awaiting a decision.**
      Past toc-depth, render fails with `Q-20-3: pandoc exited with exit
      status: 83 ... typst-brand-yaml.lua:339: attempt to index a string
      value (field 'brand')`. Root cause, confirmed via the retained
      `pandoc-input.json`: `brand: _brand.yml` (a bare path string)
      serializes to pandoc as `{"t": "MetaString", "c": "_brand.yml"}`. The
      vendored `typst-brand-yaml.lua`'s guard
      (`resources/pandoc-filters/filters/quarto-post/typst-brand-yaml.lua:261-263`,
      `if not meta.brand or pandoc.utils.type(meta.brand) == 'Inlines' then
      meta.brand = {} end`) was written for Q1's convention, where pandoc's
      own YAML-metadata-block parser always encodes scalar front-matter
      strings as `MetaInlines`. Q2 pre-parses/pre-types metadata in Rust via
      `ConfigValue` and serializes an already-built JSON AST directly,
      bypassing pandoc's own YAML parsing; `brand` isn't routed through
      markdown-parsing (it's a config/path key, not prose), so it stays a
      plain string and serializes as `MetaString` instead.

      **Not book-merge-specific** — confirmed no other Typst fixture
      anywhere in this repo (smoke-all or Rust integration tests) exercises
      `brand:` as a bare path string; the only existing coverage
      (`crates/quarto-core/tests/integration/pandoc_typst_writer.rs`) uses
      an *inline brand map*, which serializes as `MetaMap` and never hits
      this guard at all. orange-book is genuinely the first fixture to
      exercise this path, book or not — matches this item's own
      already-documented risk note above almost exactly, just one layer
      deeper (the metadata-*encoding* gap, not just the logo-path
      resolution one that note anticipated).

      **Decided (Gordon, 2026-09-28): option (1), the narrow Lua-filter
      patch.** Added `pandoc.utils.type(meta.brand) == 'string'` to the
      guard, marked `QUARTO2-PATCH`, and added the file to
      `resources/pandoc-filters/README.md`'s "Ours vs. pinned" list.
      Removed the `run.skip`. Regression tests: two new cases in
      `typst_brand.rs` (`test_logo_map_resolves_named_size_against_images`
      and `..._explicit_resource_not_treated_as_name_reference` — see bug
      5 below, added alongside). `cargo clippy -p quarto-core --all-targets
      -- -D warnings` clean; `cargo nextest run -p quarto-brand -p
      quarto-core` 5387/5387 passed.

      **Fifth bug — found and fixed 2026-09-28, immediately after
      unblocking bug 4.** Past the brand-string crash, typst compile
      failed with `error: file not found (searched at .../_book/test-logo)`
      — the fixture's `_brand.yml` sets `logo.medium: test-logo`, a name
      reference into `logo.images.test-logo` (a real, documented brand.yml
      convention: "You can also specify named logos under images which
      you can reference in small, medium and large" — confirmed against
      Q1's `getLogo`/`getLogoResource` precedence in `brand.ts`, which
      checks `images` first and only falls back to treating the string as
      a literal path). Q2's `logo_map` (`typst_brand.rs`) never resolved
      this: it took `brand.logo("medium")`'s raw `LogoEntry::Single(Path(
      "test-logo"))` and emitted `"test-logo"` as the literal path
      verbatim. Same bug in `quarto_brand::Brand::favicon()` and
      `ResolvedBrand::logo_resource_relative_to()` — neither consulted
      `images` for a bare-string `small`/`medium`/`large` value either, so
      this wasn't Typst-specific, just first-exercised there.

      Fix: added `Brand::resolve_named_logo()` (`quarto-brand/src/
      resolve.rs`) implementing Q1's precedence — a bare `Path(name)` is
      looked up against `logo.images`; a match wins, an explicit `{path,
      alt}` value is never treated as a name reference. Threaded through
      `typst_brand::logo_map`, `Brand::favicon()`, and
      `ResolvedBrand::logo_resource_relative_to()`. New tests: 2 in
      `typst_brand.rs`, 2 in `quarto-brand/tests/integration/logo_test.rs`;
      all existing `resolved_test.rs`/`logo_test.rs` cases (100/100)
      stayed green, confirming no regression to the `LightDark`/
      no-fallback semantics those already pinned.

      Once that path resolved, compile failed again — same file, now
      searched at `.../_book/logo.svg` instead of `.../_book/test-logo`.
      Root cause: a book render's compiled `.typ` (and PDF) lands in the
      project's *output* directory (`_book/`), one level below the
      project root where `_brand.yml` and `logo.svg` actually live; Typst
      resolves a relative `image()` path against the *including file's*
      own directory, not the project root. `resolve_typst_brand_param`
      (`pandoc_write.rs`) was passing `&ctx.project.dir` as the base for
      `build_brand_param`'s path rewriting — correct for a single-document
      render (output dir == project dir there, which is why this was
      invisible until a book render exercised it), wrong for a book.
      Fixed by computing `output_dir` from `ctx.output_path().parent()`
      (falling back to `ctx.project.dir` if there's no parent) and passing
      that instead. New end-to-end test:
      `book_single_file_merge.rs::book_brand_logo_named_by_images_reference_resolves_relative_to_book_output_dir`
      — a real book render (via `orange-book`, Q2's default typst-book
      extension) with `_brand.yml`'s `small: test-logo` reference,
      asserting the retained `.typ`'s logo path is `"../logo.svg"`, not
      `"logo.svg"`. `cargo nextest run -p quarto-core -E
      'test(book_single_file_merge)'` and the full `-p quarto-brand -p
      quarto-core` suite (5387/5387) both green; `cargo clippy -p
      quarto-core -p quarto-brand --all-targets -- -D warnings` clean.

      **Sixth bug — found and root-caused 2026-09-28 (bd-2lxj10z0
      filed, fixture workaround applied here).** The `fig-cars` mismatch
      (`unnamed-chunk-1-1.svg` on disk vs. `fig-cars-1.svg` expected by
      `chapter2.qmd`/`appendix.qmd`/`appendix-b.qmd`'s hand-authored
      `![...](chapter1_files/figure-typst/fig-cars-1.svg)` references)
      turned out to be **general to q2, not book-merge-specific** —
      confirmed by reproducing the identical stripped input to knitr in
      both a plain single-document render and the book-merge render (a
      direct debug capture of the markdown string handed to
      `crates/quarto-core/src/engine/knitr/resources/rmd/execute.R`
      showed `#| label: fig-cars`/`#| fig-cap:` already gone in both
      cases, replaced by a bare `::: {#fig-cars} ... :::` wrapper), and
      by confirming with a bare `rmarkdown::render()` call that knitr
      itself names figures by label correctly when given the label
      unstripped (`fig-cars-1.png`, not `unnamed-chunk-1-1.png`).

      Root cause: `PreEngineSugaringStage` /
      `crates/quarto-core/src/crossref/codeblock_shorthand.rs`
      deliberately **consumes and removes** `label:`/`<reftype>-cap:`
      from an executable code cell's body before the engine ever sees it
      (by design, D2/D7 — the label moves into a wrapping
      `::: {#fig-cars}` Div for q2's own crossref numbering instead).
      Side effect, apparently never noticed until this fixture: knitr's
      own native label-based figure-filename convention (real, confirmed
      Q1 behavior — Q1's tracked `_book/chapter1_files/` genuinely
      contains a `fig-cars-1.svg`) becomes unreachable, since knitr never
      sees the `label:` option anymore and falls back to
      `unnamed-chunk-N` auto-naming. Single-document renders never
      surface this as a *failure* — nothing downstream references the
      actual filename by name — so the regression was invisible until a
      book fixture cross-referenced a sibling chapter's generated figure
      by its Q1-convention path.

      **Gordon's call (2026-09-28):** real fix is out of P8's scope
      (affects every q2 knitr render with a labelled figure/table, not
      just this fixture) — filed as **bd-2lxj10z0** with full root-cause
      detail and a suggested fix direction (re-inject `#| label: <id>`
      into the code block text at engine-serialization time, engine-side
      only, no change to q2's own crossref/numbering). Worked around
      *here* by pointing the six affected image references at the
      filename q2 actually produces
      (`chapter1_files/figure-typst/unnamed-chunk-1-1.svg`) instead of
      the Q1-convention `fig-cars-1.svg` — see bd-2lxj10z0's comment
      trail for the exact file/line list to revert once the real fix
      lands. No test was skipped for this; index.qmd's assertions don't
      reference the filename by name.

      **Seventh bug — found and fixed 2026-09-28, immediately after
      the bug-6 workaround unblocked the render further.** Typst compile
      then failed with `error: the document does not contain a
      bibliography` / `label <...> does not exist in the document` for
      five citations (`dijkstra1968`, `hoare1978`, `pearl2009`,
      `cortes1995`, `box1976`) — all confirmed present in
      `references.json`, so the bibliography data itself was fine. Every
      one of the five lives inside a `.callout-*` Div or a custom
      crossref (`{#dino-...}`, ref-type `dino`) Div — i.e. inside a
      `CustomNode` (Callout/FloatRefTarget) scaffold by the time citeproc
      runs. Root cause, confirmed by reading the code:
      `collect_citations_from_block`/`collect_citations_from_inlines`
      and `transform_block`/`transform_inlines` in
      `crates/pampa/src/citeproc_filter.rs` had no `Block::Custom`/
      `Inline::Custom` arm — both fell through the generic `_ => {}`
      catch-all, so any `@cite` nested inside *any* custom node was
      invisible to citeproc's citation collector and never got its
      raw `Cite` node replaced. The unresolved `Cite` survived to the
      Typst writer, which emitted a native `#cite(<id>)` call that
      Typst's own `#bibliography()` (built from citeproc's — incomplete —
      resolved set) didn't know about.

      Fix: added `Block::Custom`/`Inline::Custom` arms to all four
      functions, walking `node.slots` exactly the way
      `crates/quarto-core/src/transforms/equation_label.rs` already does
      for the same shape (`Slot::Block`/`Blocks`/`Inline`/`Inlines`, each
      recursing back into the matching collect/transform function).
      `transform_inlines`'s signature changed from `&mut Vec<Inline>` to
      `&mut [Inline]` so a `Slot::Inline`'s single owned inline can be
      passed via `std::slice::from_mut` without a temporary `Vec` — a
      pure widening, no call site needed updating (`&mut Vec<T>` already
      coerces to `&mut [T]`). Two new regression tests
      (`test_collect_citations_in_custom_node`,
      `test_transform_block_in_custom_node`) build a bare `Callout`
      `CustomNode` with a `Blocks` slot containing a cited paragraph and
      assert the citation is found and replaced. `cargo nextest run -p
      pampa` (full suite): 4851/4851 passed. `cargo clippy -p pampa
      --all-targets -- -D warnings`: clean. This is a plan-scoped fix
      (found while directly blocking this same checklist item, not a
      digression) — implemented here, not filed as a bead, per
      `CLAUDE.md`'s beads-vs-plans rule.

      **Eighth bug — root-caused and fixed 2026-09-28** (ninth and tenth
      bugs surfaced and fixed along the way — see below). With
      bugs 6 and 7 both addressed, the render compiles all the way to a
      PDF for the first time, but the assertion pass surfaces a large
      new failure: 70 occurrences of `[WARN] unresolved crossref` across
      11 distinct `@sec-*` ids (`@sec-intro`, `@sec-methods`,
      `@sec-basic-figures`, `@sec-tables`, `@sec-cross-references`,
      `@sec-custom-crossref-dinosaurs`, `@sec-more-dinosaurs`,
      `@sec-embedded-notebooks`, `@sec-appendix-sub-figures`,
      `@sec-appendix-callouts`, `@sec-appendix-dinosaurs` — every `@sec-`
      id in the book except none), 184 "Required pattern not found"
      assertions, a PDF page-ordering assertion (`ALIGNTEST_MARKER` on
      page 11 vs. `LISTING_BODY_ALIGN_TEST` on page 10, likely a
      downstream symptom of the same numbering breakage), and one
      already-expected failure (`@fig-visualization`/
      `sec-embedded-notebooks`, item 3's `embed`-unimplemented gap — 3
      lines, not part of this bug).

      Root cause, confirmed by reading the code and matching it against
      the actual diagnostic text: `crates/quarto-core/src/transforms/
      crossref_index.rs`'s `visit_header` (around line 326) has
      `if unnumbered || !self.html { return; }` — for any non-HTML
      format (`self.html = ctx.format.identifier.is_html_based()`,
      false for Typst), this skips **both** the HTML-only `number` kv
      stash (correctly HTML-only — Typst does its own native heading
      numbering) **and** `sec`-target registration into the
      `CrossrefIndex` (this is the actual bug — registration is
      unconditional in Q1). The gate's own comment says the intent was
      "the pandoc-hybrid pipeline also runs this transform, and
      registering there would let crossref-resolve consume `@sec-`
      cites before the vendored refs.lua sees them" — but exhaustive
      grep across `crates/quarto-core/src` and `crates/pampa/src` found
      **no live invocation** of any file under
      `resources/pandoc-filters/filters/crossref/` (`index.lua`,
      `refs.lua`, `format.lua`, etc.) — every reference to them in the
      current Rust source is a "ported from"/"matches Q1's" comment
      citing the algorithm's origin, not a filter-chain wiring. This
      strongly suggests the "Lua handles sec for non-HTML" assumption
      is stale for Q2's current architecture (crossref is now fully
      native Rust), though this hasn't been independently confirmed for
      every pandoc-hybrid format (only Typst was checked in depth).

      Confirmed via the actual diagnostic path: `crossref_resolve.rs`'s
      `resolve_one_cite` classifies `@sec-intro` as ref-type `sec` via
      the format-agnostic `RefTypeRegistry` (unaffected by the `!self.html`
      gate — that gate only controls the per-document *index* of real
      targets, not the registry of valid ref-type prefixes), looks it
      up in the `CrossrefIndex`, finds nothing (never registered, per
      above), and emits exactly `"unresolved crossref \`@{id}\`: no
      target with this identifier was found."` — the literal string
      seen in the smoke-test output. `render_resolved_ref`
      (`crossref_render.rs`) is documented to render an unresolved ref
      as literal `"?id?"`, but the actual `.typ` output shows bare
      `sec-intro` with no `?` wrapping — an open discrepancy not yet
      explained. **This doesn't block the fix**: once `sec` targets are
      actually registered, resolution succeeds and the `resolved: true`
      branch (already proven correct — every other ref type: fig, eq,
      thm, lst, and the custom `dino` type all already render as proper
      `#ref(<id>, supplement: [...])` in this exact book) takes over;
      the unresolved-rendering discrepancy only matters if some `@sec-`
      id still fails to resolve after the fix.

      An existing test pins the *current* (to-be-changed) behavior:
      `crossref_index.rs::sec_registration_is_html_only` (`Format::pdf()`,
      asserts `idx.get("sec-a").is_none()` with comment "pandoc-hybrid
      formats keep Lua-native @sec- resolution") — this test encodes the
      assumption above and will need to be rewritten, not just left
      failing, as part of the fix.

      **Fixed 2026-09-28.** Scope decision: apply to **every format, not
      just Typst** — dispatched a fork to check whether LaTeX/PDF has an
      independent, already-working `@sec-` resolution path before
      generalizing. Finding: `FormatIdentifier::Pdf` is neither
      `is_native()` nor `is_pandoc_hybrid()` (`format.rs`'s own comment:
      "Pdf is deliberately absent: the latex/beamer epic owns it") — PDF
      has **no working render pipeline in q2 at all yet**, so there is
      nothing today that could depend on `sec` staying unregistered for
      it. The pandoc-hybrid Typst path's only Lua involvement
      (`quarto2-shim.lua`'s `route_crossref_resolved_ref`) merely formats
      an *already-resolved* ref per-writer; it never builds the
      `CrossrefIndex` or registers `sec` targets — that is 100% owned by
      the Rust `crossref_index.rs`, run uniformly before dispatch to any
      writer. So the "Lua handles it" assumption was stale for every
      non-HTML format, not just Typst. Implemented: `visit_header`
      (`crossref_index.rs`) now splits the old combined
      `if unnumbered || !self.html { return; }` gate — `unnumbered` still
      short-circuits everything, but the HTML-only condition now guards
      *only* the `number` kv stash; `sec`-target registration runs
      unconditionally. Rewrote `sec_registration_is_html_only` →
      `sec_registration_is_format_agnostic` (asserts the opposite:
      `Format::pdf()` now registers `sec-a`). `cargo nextest run -p
      quarto-core`: 5287 tests, all green (0 failed, 32 skipped).

      Re-running the smoke test after this fix alone dropped `@sec-*`
      unresolved-crossref warnings from 70 to 0 (only the already-known
      `@fig-visualization` embed-gap warning remains), but surfaced a
      **ninth bug**: `@sec-*` refs for a chapter-level heading rendered
      with supplement `[Section]` instead of the expected `[Chapter]`
      (`#ref(<sec-intro>, supplement: [Section])` vs `[Chapter]`). Root
      cause: Q1's `refs.lua`'s chapter/appendix prefix-type swap
      (`isChapterRef` + `crossrefOption("chapters", false)`, `refs.lua:
      60-70`) was never ported into `quarto2-shim.lua`'s
      `route_crossref_resolved_ref` — its own doc comment explicitly
      flagged this as a "v1 scope-out." Ported it (mirrors Q1 exactly,
      using `data.order`/`data.in_appendix`, always present when
      `data.resolved == true`). That alone still rendered `[ch.]` instead
      of `[Chapter]`, because `param("crossref-ch-prefix")` was never
      populated for the Lua filter chain: `insert_crossref_title_prefix_family`
      (`crossref_params.rs`) only emits `-title`/`-prefix` params for
      ref-types in the `RefTypeRegistry`, and `ch`/`apx` are synthetic
      presentation keys, not registered ref-types. Added an explicit,
      unconditional second loop emitting `crossref-ch-title`/`-prefix`
      and `crossref-apx-title`/`-prefix` from `LanguageTerms` (which,
      unlike the Lua param blob, already covers every `_language.yml`
      key). Updated `test_both_crossref_families_are_emitted` (now
      asserts the 4 synthetic keys too) and the `params_blob_key_set`
      insta snapshot. Both `@sec-*` resolution and the Chapter/Appendix
      supplement swap are now fully correct in the rendered `.typ`/PDF.

      Re-running again surfaced a **tenth bug**, orthogonal to crossref
      entirely: ~178 PDF-text "Required pattern not found" failures, an
      exact byte-for-byte repeat of the pre-existing baseline (confirmed:
      the same 85 `ensurePdfRegexMatches` mismatches occurred before
      *and* after the `sec` fix). Root cause (confirmed by dumping
      `pdf_extract::extract_text` output directly): the `pdf-extract`
      crate inserts a spurious extra space at Typst content-box
      boundaries — e.g. a numbered `#ref()`/caption-prefix box abutting
      literal surrounding text produces `" Figure 1.1:  A plot..."`
      instead of `"Figure 1.1: A plot..."`, and similarly a bare space
      before trailing punctuation like `. , :` where a ref box is
      immediately followed by punctuation in the source
      (`"Chapter 1. , we now present"`). This is a known
      PDF-text-extraction artifact, not a content bug — **Gordon's
      guidance: "we generally do not worry about the number of spaces
      ... we just fix the test to accept any amount of whitespace."**
      Confirmed via a fork that orange-book is the *only* fixture using
      `ensurePdfRegexMatches` (no prior convention to match) and that the
      existing ad hoc `\s+` spots in this same file were already an
      instance of the same fix, just applied inconsistently. Fixed by
      mechanically replacing every literal run of spaces with `\s+`
      across all of `ensurePdfRegexMatches` (not just the failing ones),
      plus `\s*` immediately before trailing punctuation (`,`/`:`/`.`)
      that directly abuts a ref-rendered number/word with no space in
      the original pattern. (First attempt used a single backslash —
      `\s+` — which is not a valid YAML double-quoted-scalar escape and
      broke frontmatter parsing; fixed to the correct double-backslash
      `\\s+` encoding, verified by re-parsing.) Also fixed one incidental
      stale pattern found along the way: `path: "logo\.svg"` should have
      read `path: "\.\./logo\.svg"` since the book-output-dir fix
      earlier in this plan (the brand-logo-resolution fix) intentionally
      changed the correct value to `"../logo.svg"`, but the test pattern
      was never updated to match.

      **Remaining failures after both fixes — all pre-existing, already
      tracked, and out of scope for this bug:**
      - `<fig-visualization>` / its `#ref()` / "Figure 1.2: A display of
        a line" / "Some content in Section 1.2. See Figure 1.2" — item
        3's `embed`-shortcode-unimplemented gap (separate epic).
      - `Turing\s+\(1950\)\s+on\s+machine\s+intelligence` and 3 sibling
        citation patterns (McCarthy/Codd/Lamport) — rendered as
        `"(Turing 1950)"` (parenthetical) instead of `"Turing (1950)"`
        (author-in-text, the correct rendering for a bare `@turing1950`
        citation). Not investigated further — very likely a symptom of
        the *already-tracked* temporary `references.json` workaround
        (bd-l6eh1635 BibTeX blocker) losing CSL fields needed for
        correct in-text-vs-parenthetical citation-mode rendering; this
        plan's own "Next steps (2)" already calls for re-verifying once
        `references.bib` is restored, which should be the right moment
        to re-check this too.
      - `outline-depth: 17,` (actual: `outline-depth: 3,`, Typst's
        native default) — **newly surfaced, not yet diagnosed.** The
        earlier `toc-depth` fix in this same plan (`format_defaults.rs`
        skips `--toc-depth` CLI forwarding for Typst, relying on the
        `$toc-depth$` template variable) apparently doesn't reach the
        book-merged document's metadata correctly. Needs its own
        investigation — not attempted here, out of scope for this bug.
      - `ensurePdfTextPositions`: `ALIGNTEST_MARKER` still on page 11 vs
        `LISTING_BODY_ALIGN_TEST` on page 10 — unchanged by either fix
        above; still the "likely a downstream symptom" item flagged
        earlier in this section, still not independently investigated.
      - `noErrorsOrWarnings`: 112 warnings (HTML-raw-passthrough,
        metadata-as-markdown parse failures, the `embed` shortcode
        warning, the `@fig-visualization` unresolved-crossref warning) —
        this fixture has never been warning-free; reaching zero requires
        either fixing each underlying warning or the fixture opting into
        an explicit `printsMessage`/`noErrors` acknowledgment list,
        neither attempted here.

      **Next steps, in order:** (1) decide whether to chase the
      newly-surfaced `outline-depth`/book-merge toc-depth bug and the PDF
      page-ordering symptom now or file them for later — both are
      genuinely new ground, not part of this bug's scope. (2) once
      bd-l6eh1635 actually merges into `feature/typst-testing`, rebase
      this branch onto it, swap `references.json` back out for
      `references.bib` in `_quarto.yml`, delete `references.json`, and
      re-verify — including the citation-mode mismatch above.
- [x] Cross-check against the six existing Rust integration tests
      (`book_numbering_torture.rs` et al.) — any assertion that fails here but passes
      there points at a smoke-all-harness gap, not a rendering regression; triage
      accordingly before assuming a Q2 bug.

      **Baseline confirmed green 2026-09-28**, unaffected by this branch's fixture
      work: `cargo nextest run -p quarto-core -E 'test(book_numbering_torture) +
      test(book_multifile_bibliography) + test(book_appendix_letter_parity) +
      test(orange_book_lua) + test(book_citations) + test(book_theorem_crossref)'`
      → 15/15 passed (1 leaky, pre-existing). Real cross-checking against
      smoke-all assertion failures can't happen until item 4 unblocks and the
      fixture actually renders.
- [x] `cargo clippy -p quarto --all-targets -- -D warnings` + `cargo nextest run
      -p quarto`.

      Both green as of 2026-09-28 (clippy clean; nextest 602 passed, 2 skipped —
      see item 4 for why orange-book's own test is one of the skips).

## Status

**In progress — five real, separate, verified bugs found so far, all five
fixed.** Blocked next on a sixth (see below), still inside item 4's "finish
rendering the whole book" work — not a digression. The BibTeX blocker
(bd-l6eh1635, workspace-6/`braid/bd-l6eh1635-bibtex-citeproc`) is temporarily
worked around (a local `references.json` conversion, reversible, see item 4)
rather than actually resolved — that dependency still needs to land and be
swapped back in before this branch is done.

1. **Fixed**: `single_file_render.rs` ran citeproc *before* the merged
   document's Crossref phase instead of after, unlike the single-document
   `.post`-bucket convention. Regression-checked against 63 book-related
   tests plus the full `-p quarto-core` suite (5282 tests), all green.
2. **Fixed**: `format_defaults.rs` forwarded `--toc-depth` to pandoc's CLI
   for Typst, which hard-validates 1-6, even though Typst never consumes
   that CLI-driven mechanism (verified against both Q2's default template
   and every vendored extension's template — TOC depth reaches Typst purely
   via a `$toc-depth$` template variable). Skipped `--toc`/`--toc-depth`
   forwarding for `FormatIdentifier::Typst`, matching this file's existing
   format-conditional-exception pattern. Two new tests, both green.
3. **Fixed (Gordon decided option 1, 2026-09-28)**: `brand: _brand.yml`
   (bare path string) serialized to pandoc as `MetaString`; the vendored
   `typst-brand-yaml.lua`'s guard only recognized `MetaInlines`. Patched the
   guard to also accept `pandoc.utils.type(meta.brand) == 'string'`
   (`QUARTO2-PATCH`, documented in `resources/pandoc-filters/README.md`).
   Removed the `run.skip`.
4. **Fixed**: `logo.medium: test-logo` (a name reference into
   `logo.images`, a real documented brand.yml convention) was treated as a
   literal path everywhere in `quarto-brand` — `typst_brand::logo_map`,
   `Brand::favicon()`, `ResolvedBrand::logo_resource_relative_to()`. Added
   `Brand::resolve_named_logo()` implementing Q1's `getLogoResource`
   precedence (images-name match wins over literal-path fallback) and
   threaded it through all three call sites. 4 new tests; all 100 existing
   `quarto-brand` tests stayed green.
5. **Fixed**: even after (4), the resolved logo path was still wrong for a
   *book* render — `resolve_typst_brand_param` rewrote brand-relative paths
   against `ctx.project.dir`, but a book's compiled `.typ`/PDF lands in the
   project's output directory (`_book/`), one level down, and Typst
   resolves relative `image()` paths against the including file's own
   directory. Fixed by rewriting against `ctx.output_path().parent()`
   instead (a no-op for single-document renders, where output dir ==
   project dir — why this was invisible until a book render exercised it).
   New end-to-end test in `book_single_file_merge.rs` renders a real
   branded book and asserts the retained `.typ`'s logo path.
   `cargo nextest run -p quarto-brand -p quarto-core`: 5387/5387 passed;
   clippy clean on both crates.

**Sixth bug — found, not yet fixed, start here on resume.** Past all five
logo/brand fixes, typst compile now fails on
`error: file not found (searched at .../_book/chapter1_files/figure-typst/fig-cars-1.svg)`.
`chapter1.qmd`'s R chunk explicitly sets `#| label: fig-cars`, but the
actual generated file is `unnamed-chunk-1-1.svg` — the chunk label isn't
reaching the R/knitr figure-naming convention in this book-merge render.
Not yet root-caused (single-document renders with a labelled chunk need
checking as a comparison point — is this book-merge-specific like bugs
1/5, or general?).

Also resolved the `requires: jupyter` question from the plan (not needed —
`embed` is unimplemented, own epic, per D6). Fixture itself is reconciled to
Q1's tracked source (plus one required Q-2-7 apostrophe-escape deviation),
`render-project: true` confirmed, existing Rust integration-test baseline
confirmed green. Not merging to `feature/typst-testing` yet — nothing here is
ready to flip to `Complete.` until item 4 actually runs end to end.
