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
- [ ] Finish triaging the 86-file run into (a) predicate bugs — feed back into P3,
      (b) real Q2 Typst rendering gaps, out of this epic's scope, and (c) fixture
      syntax-translation mistakes — fix in this port. Latest focused run: 24 passed,
      10 skipped, 52 failed; see §Decisions for the grouped failure causes.
- [ ] Close the remaining Q2 format/filter/render gaps listed in §Decisions, then
      rerun the P5 smoke suite and update this checklist.
- [x] `cargo clippy -p quarto-core --all-targets -- -D warnings`,
      `cargo clippy -p quarto --all-targets -- -D warnings`, and
      `cargo fmt --all -- --check` pass at commit `17871afad`.
- [ ] `cargo nextest run -p quarto-core` passed 5283 tests, 32 skipped before the
      final fixture edits. Latest `cargo nextest run -p quarto` is red because
      smoke-all includes the 52 remaining P5 failures plus the existing
      `typst/pdf-text-position-test.qmd` failure; live total: 601 passed, 1 failed,
      2 skipped. Rerun after fixes.

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
- Remaining decisive gaps: Pandoc receives `--reference-location margin` and exits
  with “Argument of --reference-location must be block, section, or document” for
  five sidenote/mixed fixtures. `layout/meta.lua` and `quarto-post/typst.lua` read
  `reference-location` and `citation-location` from `QUARTO_FILTER_PARAMS`, but
  `FilterParamsBuilder` does not currently pass these metadata values; margin
  citation output therefore lacks the expected `#note(... form: "full")`. Fix the
  Typst metadata bridge and avoid forwarding the unsupported `reference-location:
  margin` CLI option to Pandoc.
- `citation-margin-multiple.qmd` additionally fails Typst compilation because the
  generated source treats `@vindication1738---a` as a reference label that does not
  exist. Most other remaining failures are real output-vs-assertion differences in
  margin geometry, captions/listings, figures/tables, and pagination; classify after
  the format/filter bridge is corrected. Do not weaken or delete the source assertions
  just to make the suite green.
- The standalone pre-existing fixture `typst/pdf-text-position-test.qmd` currently
  fails because the header/footer decorations are on page 1 while the body/title are
  on page 2. It contributes the extra non-P5 failure in the full `quarto` test run.

## Status

In progress — fixture port and smoke-all discovery are complete; P5 triage has
started. Current branch checkpoint is `typst-testing/p5-margin-layout`. Do not merge
or push until the remaining P5 failures are classified and the phase test gates pass.
