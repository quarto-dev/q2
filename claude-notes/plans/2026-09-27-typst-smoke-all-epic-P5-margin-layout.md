# P5 — Port `margin-layout` (86 files, website-type project)

**Date:** 2026-09-27
**Epic:** [`2026-09-27-typst-smoke-all-epic.md`](2026-09-27-typst-smoke-all-epic.md) —
read "Decided" items 7 and 8 first: this fixture is in scope, and it's deliberately
sequenced *before* `orange-book-margin` (P9) so the struct-tree implementation gets
proven against a large, real assertion set before the harder book-context case.
**Depends on:** P1, P3, P4.
**Worktree:** `workspace-2` (Track A, sequential after P4 — see epic's "Parallel
development plan"). This is Track A's last phase.

## Worktree & git workflow

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3
git show feature/typst-testing:claude-notes/plans/2026-09-27-typst-smoke-all-epic-P4-standalone-fixtures.md \
  | grep -q '^Complete\.' && echo "P4 merged" || echo "STOP: P4 not yet merged, wait"
```

Once it prints "merged":

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-2
git checkout -B typst-testing/p5-margin-layout feature/typst-testing
```

Implement the checklist below, gating on `cargo clippy -p quarto --all-targets --
-D warnings` + `cargo nextest run -p quarto`. When done, flip this doc's checklist
to `[x]` and `## Status` to `Complete.`, commit, then:

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-2
git rebase feature/typst-testing
cd /Users/gordon/src/q2/.worktrees/workspace-3
git checkout feature/typst-testing
git merge --ff-only typst-testing/p5-margin-layout   # retry from rebase (in workspace-2) if not a fast-forward
cargo nextest run --workspace                  # phase-boundary gate
```

`workspace-2` is now free — check whether `workspace-5` still needs help finishing
P8 (see P9's doc for the exact "is P8 merged yet" check) before picking up P9.

## Why this fixture, and why it doesn't need P6

`external-sources/quarto-cli/tests/docs/smoke-all/typst/margin-layout/` — confirmed by
direct grep, not assumption:

- `_quarto.yml`: `project: type: website`. `ProjectKind::Website` is already a
  first-class, fully-handled kind in `crates/quarto-core/src/project/mod.rs:311-343`
  (sidebar/navbar/`_site` logic lives inline there — no `project/website/` submodule
  the way `project/book/` exists, because none of that logic matters when rendering a
  single file to Typst).
- 86 `.qmd` files. **76 use `ensurePdfTextPositions`**, 82 use
  `ensureTypstFileRegexMatches`, 63 use `ensurePdfRegexMatches` — by far the most
  exhaustive real-world exercise of the position predicate in Q1's entire smoke-all
  suite (more assertions than `orange-book-margin`'s ~24).
- **Zero files use `render-project: true`, zero use `run: skip`.** Each file is fully
  self-contained: own `format:` block, own assertions, own `bibliography:` reference
  where needed (e.g. `citation-margin-basic.qmd` sets `bibliography: borges-refs.bib`
  directly in its own front matter, not inherited from a project default). This is
  exactly the "single-file-in-project" render path that already works today
  (`render_to_file.rs:260`'s `ProjectContext::discover` call) — **no book-merge or
  `render-project` dedup machinery (P6) is a dependency for this phase.**

Coverage highlights across the 86 files: captions (above/below/interleaved), figures
(including `ggplot2`/svg subfigures), tables (`gt`, `flextable`, `great-tables`),
citations (bare/citeproc/elaborate/locator/suppressed, all in-margin), sidenotes
(basic/multipara/multiple/code-block), collision avoidance, shift behavior
(avoid/fixed/ignore/mixed), multiple page geometries (A4/legal/custom
narrow/wide/asymmetric), column widths, and fullwidth spans (div/figure/listing/table/
nested/screen-inset variants).

## Checklist

- [x] Copy the fixture directory's **tracked source files** into
      `crates/quarto/tests/smoke-all/typst/margin-layout/` (86 `.qmd` files,
      `borges-refs.bib`, and the four image assets). The destination `.gitignore`
      excludes generated Typst/HTML/PDF outputs, `_site/`, `.quarto/`, and copied
      local `.claude/` settings; only tracked fixture sources are committed.
- [x] Smoke-all discovers all 86 QMD files without a harness change. The website
      fixture's output is directed to the source directory so Typst can resolve its
      relative bibliography file; see §Decisions.
- [x] Triage the 86-file run into (a) predicate bugs — feed back into P3, (b) real Q2
      Typst rendering gaps, out of this epic's scope, and (c) fixture syntax-translation
      mistakes — fix in this port. After the metadata/CLI bridge, the live run is 37
      passed, 10 skipped, 39 failed; the 39 failures sort into five root-cause groups
      (citation filter/citeproc mismatch, layout/predicate boundary, margin figures/
      captions/listings, pagination/content/geometry, and one isolated mediabag crash),
      all detailed with direct evidence in §Decisions. No fixture syntax-translation
      mistakes were found among the 39 — every remaining failure is either a real Q2
      Typst-filter/rendering gap or needs a dedicated P3-predicate-vs-layout
      investigation pass; none are (c).
- [ ] Close the remaining in-scope Q2 format/filter/render gaps listed in §Decisions,
      then rerun the P5 smoke suite and update this checklist. **Scope conflict,
      flagged rather than silently resolved:** three of the five groups are
      unimplemented Q2 Typst-filter capabilities (Typst-native citeproc mode for margin
      citations; `#notefigure`/margin-caption support entirely absent from
      `quarto-post/typst.lua`; `mediabag-dir` filter-param wiring, a repo-wide gap not
      Typst-specific), not fixes scoped to this metadata-bridge pass — each is its own
      multi-file change with its own test plan. The column-width geometry group needs
      a dedicated P3-predicate-vs-rendered-layout investigation before a root cause is
      even assigned. None of this was attempted here to avoid rushing a large surface
      under gate pressure; see `## Status` for the explicit decision this leaves for
      Gordon.
- [x] `cargo clippy -p quarto-core --all-targets -- -D warnings`,
      `cargo clippy -p quarto --all-targets -- -D warnings`, and
      `cargo fmt --all -- --check` pass after the metadata-bridge change.
- [x] `cargo nextest run -p quarto-core`: 5283 passed, 32 skipped — unchanged from the
      prior baseline, confirming the bridge change is safe for quarto-core.
- [ ] `cargo nextest run -p quarto` (and the workspace phase-boundary gate) remain red
      because `smoke_all` still contains the 39 P5 failures above plus the pre-existing
      `typst/pdf-text-position-test.qmd` failure. Not rerun to a clean state in this
      session — the 39 failures are the classified gaps above, not something to chase
      down further without picking one to actually fix.

## Decisions

- `render_to_file(input, format, ...)` treats its explicit format argument as
  authoritative. It previously passed `format_override: None` to the lower-level
  renderer, allowing multi-format frontmatter to win and produce HTML for a Typst
  test. It now passes `Some(format)`; the regression
  `render_to_file_typst_overrides_multiple_frontmatter_formats` proves a real PDF
  is produced. The focused regression passes.
- The smoke-all test spec uses each `_quarto.tests` format key as the render format;
  no new per-test `format` metadata option or assertion-specific PDF forcing is
  needed. The observed `.html` mismatch was a renderer precedence bug, not a
  `quarto-test` selection bug.
- The website project's Q2 default output directory is `_site/`, but Typst source
  keeps local resource names such as `#bibliography(("borges-refs.bib"))`. Q2 does
  not copy the BibTeX asset into `_site/`; the fixture sets `project.output-dir: .`
  so the compiled `.typ` and its declared bibliography share the source directory.
  This is a fixture-local accommodation, not a change to website output defaults.
- Nine Python/Great Tables fixtures declare `tests.run.requires: jupyter`; this
  machine has no Jupyter runtime, so they correctly skip rather than fail as engine
  errors. R/knitr is available here; R fixtures remain active and expose their
  actual rendering gaps.
- Q1 syntax that Q2 explicitly rejects was translated in the copied QMDs: listing
  attributes put classes before key/value pairs; Legal-paper dimensions use words
  instead of unmatched inch quotes; multiline footnote definitions use Q2's `:::
  ^label` block form instead of unsupported Pandoc indentation. The suite progressed
  past those parser errors.
- Metadata/CLI gap fixed and directly verified: `PandocWriteStage` now places
  `citation-location` and `reference-location` from merged metadata in the Typst
  contributor; `build_forwarded_args` no longer sends Typst `reference-location` to
  Pandoc (but still forwards valid `block`/`section`/`document` values to other
  writers). An end-to-end Typst source regression confirms both margin settings have
  observable effects; a unit regression checks the format-gated CLI args. Focused tests
  pass.
- The post-bridge run is 37 passed, 10 skipped, 39 failed. Current failure classes:
  - **Citation filter/citeproc mismatch (7 fixtures):** regular margin citations now
    reach the filter, but their original Pandoc Cite node serializes inline to a native
    `@citation` reference before the appended full-citation note; Q1's citeproc mode
    also expects its `Pandoc` preprocessing pass to populate `citeprocBibliography`,
    and suppression is not reflected in `suppress-bibliography` because the existing
    Typst template emits `#show bibliography: none` even with the source option. The
    generated `.typ` evidence shows the filter gets the new params, while expected
    author/year, locator, and full-citation strings are absent in the PDFs. These are
    Q2 Typst filter/template behavior gaps, not fixture syntax issues.
  - **Layout/predicate boundary:** column width fixtures fail position relations
    consistently: e.g. `column-widths-both` reports edge locations for `OUTSET-B`,
    `PINSET-B`, `PAGE-B` in reverse order from intended right-edge extension, and the
    left/right variants similarly disagree. `fig-column-margin` also reports the
    table to the right of body text where the fixture expects it to remain within the
    body. These require inspecting the output geometry and Q1 predicate equivalence
    before deciding whether predicate or layout behavior is at fault; no assertions
    have been weakened.
  - **Margin figures/captions/listings (18 fixtures):** Typst output has no
    `#notefigure(...)` wrappers, no caption-in-margin show rule for `cap-location`,
    and regular figure/table/listing caption labels and references are absent in the
    PDFs. `quarto-post/typst.lua` currently handles `.column-margin` Div/Span only;
    it has no margin-float/caption path. These are concrete Q2 Typst filter gaps, not
    translated fixture syntax.
  - **Pagination/content and geometry assertions (several fixtures):** multiple
    expected same-page layout pairs land on different pages (including figure caption
    ordering); code-block content in a sidenote and some source phrases expected in
    the PDF are missing; custom-geometry and width placement assertions remain red.
    Need further classification against generated `.typ` and the source AST before
    ruling out fixture translation or P3 predicate behavior.
  - **Remaining isolated errors — root cause identified:** `crossref-grand-finale.qmd`
    (the only fixture in this set using remote `https://placehold.co/...` images)
    crashes inside `modules/mediabag.lua`'s `write_mediabag_entry`: `param("mediabag-dir",
    nil)` returns `nil` because `FilterParamsBuilder`/`PandocWriteStage` never emit a
    `mediabag-dir` key at all — confirmed by direct grep, zero occurrences anywhere in
    `crates/quarto-core/src`. `pandoc.path.join{nil, src}` then throws "string expected,
    got nil" from `quarto-finalize/mediabag.lua`'s `Image` handler, which calls this
    unconditionally for any non-Office Pandoc-hybrid format (not Typst-specific — docx/
    pptx/etc. would hit the same crash the first time a fixture fetches a remote image).
    Q1's equivalent (`command/render/filters.ts:571`, `render.ts:119-120`) sets this to
    `<stem>_files/mediabag` next to the source, created up front and treated as a
    supporting file for copy/cleanup — matching Q2's directory lifecycle for this is a
    distinct, moderately-sized wiring task of its own (new builder key + directory
    creation/supporting-file registration), not a param-bridge one-liner. This is a
    genuine, specifically-identified Q2 Pandoc-shim gap that blocks exactly this one
    fixture; out of scope for this metadata-bridge pass. The original
    `citation-margin-multiple` invalid-label failure from the prior checkpoint is gone,
    confirming the metadata fix changed the failure set as expected.
- The 39-fixture classification is complete at the category level (five root-cause
  groups above); several groups (citeproc-mode margin citations, margin-float/caption
  `#notefigure` support, `mediabag-dir` wiring) are themselves unimplemented Q2 Typst-
  filter capabilities, not quick fixes, and the column-width geometry group needs a
  dedicated investigation pass against Q1's predicate semantics before assigning root
  cause. None of these can be closed as a continuation of the metadata-bridge fix in
  this same work session without materially expanding scope; see `## Status` for the
  explicit scope-conflict flagged for Gordon. Generated `.typ` artifacts remain
  available under `crates/quarto/tests/smoke-all/typst/margin-layout/` (the project's
  `_quarto.yml` directs output to the source tree) for whoever picks up each category.
- The standalone pre-existing fixture `typst/pdf-text-position-test.qmd` currently
  fails because the header/footer decorations are on page 1 while the body/title are
  on page 2. It contributes the extra non-P5 failure in the full `quarto` test run.

## Status

In progress — fixture port, smoke-all discovery, and the metadata/CLI bridge fix are
complete and gated (`quarto-core` clippy + fmt + 5283/5283 nextest all pass, unchanged
from baseline). Triage of the 39 remaining failures is complete at the category level:
five root-cause groups, each backed by direct evidence in §Decisions.

**Scope conflict for Gordon to resolve, not silently decided here:** the P5 plan's
acceptance criteria (Checklist) call for closing "the remaining Q2 format/filter/render
gaps" before rerunning to a clean-or-explicitly-filed-exception state. Three of the
five failure groups are, on inspection, unimplemented Q2 Typst-filter capabilities of
real size — not gaps this metadata-bridge pass can absorb:

1. Typst-native citeproc mode for margin citations (`citeproc: true` currently has no
   effect on Typst rendering at all — confirmed `cite-method` is never emitted into
   `QUARTO_FILTER_PARAMS`, and even if it were, Q2's own pre-stage citeproc pipeline
   would need to interoperate with `quarto-post/typst.lua`'s margin-note Cite handler
   in a way that doesn't exist today).
2. `#notefigure`/margin-caption support: `quarto-post/typst.lua` only handles
   `.column-margin` Div/Span; there is no margin-float or `cap-location: margin`
   caption path at all. Affects the largest failure group (figures/tables/listings).
3. `mediabag-dir` filter param: never wired anywhere in `crates/quarto-core/src`,
   causing a Lua crash (not an assertion failure) for any Pandoc-hybrid render — not
   just Typst — that fetches a remote image. Repo-wide gap, surfaced here by one
   fixture.

A fourth group (column-width geometry: `column-widths-{left,right,both}`,
`fig-column-margin`) needs its own investigation pass comparing Q1's predicate
semantics against the actual rendered geometry before even a root cause (P3 predicate
vs. Q2 layout) can be assigned — deliberately not guessed here.

Given this, P5 cannot honestly be marked complete in this session without either (a)
implementing three separate, non-trivial Typst-filter capabilities plus one
investigation pass, or (b) Gordon explicitly descoping specific fixtures/categories
under the epic's own "N/86 pass, rest filed as identified gaps" allowance (epic doc,
Missing-test-pass section). Recommendation: pick option (b) and continue on a
per-category basis in follow-up work, since each category is itself plan-sized.
Current branch checkpoint is `typst-testing/p5-margin-layout`. Do not merge or push
until Gordon has made this call and the phase test gates pass against whatever
acceptance criteria result.
