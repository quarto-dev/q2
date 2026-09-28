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

      Two fix shapes discussed with Gordon, not yet decided: (1) narrow —
      patch the vendored Lua guard to also accept `pandoc.utils.type(...) ==
      'string'`, using this project's established `QUARTO2-PATCH` convention
      for tracked, justified deviations from the vendored Q1 filter chain;
      scoped to the one known-affected filter. (2) broader — change Q2's
      `ConfigValue`→`MetaValue` serialization so plain scalar strings always
      emit `MetaInlines`, matching Q1/pandoc's convention globally; fixes
      this class of bug everywhere at once but is a core, shared-
      serialization change with unsurveyed blast radius across every Lua
      filter/template touching string metadata. Re-added a documented
      `run.skip` on `index.qmd` for this blocker (superseding the resolved
      toc-depth skip) — `cargo nextest run -p quarto` green again (602
      passed, 2 skipped).

      **Next steps, in order:** (1) decide and land the brand-encoding fix;
      (2) finish rendering the whole book and reconcile all ~250 assertions;
      (3) once bd-l6eh1635 actually merges into `feature/typst-testing`,
      rebase this branch onto it, swap `references.json` back out for
      `references.bib` in `_quarto.yml`, delete `references.json`, and
      re-verify. Expect the `fig-visualization`/`sec-embedded-notebooks`
      assertions (3 lines, see item 3) to still fail on `embed`'s absence;
      everything else is new ground.
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

**Blocked on deciding and landing the brand `MetaString`/`MetaInlines`
mismatch fix (see item 4) before the whole-book render can complete.** This
is in-scope P8 work, not a digression — it's exactly the kind of gap porting
Q1's real smoke-all fixtures into Q2 exists to surface, not something to defer
past this branch. The BibTeX blocker (bd-l6eh1635, workspace-6/
`braid/bd-l6eh1635-bibtex-citeproc`) is temporarily worked around (a local
`references.json` conversion, reversible, see item 4) rather than actually
resolved — that dependency still needs to land and be swapped back in before
this branch is done.

Three real, separate, verified bugs found so far, two fixed:
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
3. **Not yet fixed, awaiting a decision — start here on resume.** `brand:
   _brand.yml` (bare path string) serializes to pandoc as `MetaString`; the
   vendored `typst-brand-yaml.lua`'s guard only recognizes `MetaInlines`
   (Q1's convention), so `meta.brand` survives as a raw string and crashes
   at `meta.brand.typography = ...`. Confirmed this is genuinely first-time
   territory (no other Typst fixture anywhere in this repo exercises `brand:`
   as a bare path). Two fix shapes on the table — narrow (patch the vendored
   Lua guard, `QUARTO2-PATCH` convention) vs. broad (make Q2's `ConfigValue`→
   `MetaValue` serialization always emit `MetaInlines` for scalar strings,
   matching Q1/pandoc globally) — full writeup in item 4 above.

Also resolved the `requires: jupyter` question from the plan (not needed —
`embed` is unimplemented, own epic, per D6). Fixture itself is reconciled to
Q1's tracked source (plus one required Q-2-7 apostrophe-escape deviation),
`render-project: true` confirmed, existing Rust integration-test baseline
confirmed green. Not merging to `feature/typst-testing` yet — nothing here is
ready to flip to `Complete.` until item 4 actually runs end to end.
