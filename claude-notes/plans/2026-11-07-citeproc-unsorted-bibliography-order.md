---
title: 'citeproc: unsorted styles order the bibliography by file order (bd-mumobar8)'
date: 2026-11-07
description: 'For CSL styles without a bibliography sort, list entries by citation number, as Pandoc does, instead of bibliography-file order.'
status: done  # implemented on braid/bd-mumobar8-citeproc-unsorted-styles-order
braid:
  strand: bd-mumobar8
---

**Branch:** `braid/bd-mumobar8-citeproc-unsorted-styles-order` (topic branch in the main checkout, based on `main` @ `e95a3efbf`; no PR, per the user)

## Triage verdict

**Ready to design.** The bug reproduces at HEAD. Pandoc's behavior is pinned down empirically. No test in the CSL conformance suite distinguishes the two orderings. The fix is a local change to the no-sort branch of the bibliography generator.

## Issue context

bd-mumobar8 (P2 bug), filed 2026-11-07 during the issue #1109 triage. For a CSL style without a bibliography `<sort>` (IEEE, for example), `generate_bibliography_to_outputs` in `crates/quarto-citeproc/src/types.rs` keeps the insertion order of `self.references`, which is bibliography-file order. Citing `[@second]` and then `[@cited]` lists `[2]` before `[1]`. The strand asked us to check the CSL conformance suite before touching the citeproc crate.

## Dependency graph

- **related**: bd-o5zfde39 (closed). This is the #1109 fix (PR #1116), which made the bibliography list only cited and `nocite` references. It also gave uncited `nocite` references citation numbers after the cited ones, in load order. As a result, every listed entry now has the right number at HEAD; only the order is wrong.
- **related**: bd-citeproc-cluster-collapse-kns8uggw (open, P3). It covers a separate difference from Pandoc that shows up in the same fixtures: punctuation stays outside quotes (`work”,` where Pandoc prints `work,”`). See "Incidental finding" below.
- No `blocks` or `discovered-from` edges, so nothing depends on this strand.

## What the code looks like today

- `crates/quarto-citeproc/src/types.rs:554` (`generate_bibliography`) and `:615` (`generate_bibliography_to_outputs`) are near-duplicates. Both have `let final_ids = if sort_keys.is_empty() { ids } else { … }` (lines 572 and 641), where `ids` is `self.references.keys()` in insertion order.
- `Processor` already tracks `initial_citation_numbers`, which `get_initial_citation_number` assigns in first-cited order. The #1109 change in pampa (`apply_citeproc_filter`) also numbers the uncited `nocite` references, in load order.
- Book projects: `build_merged_bibliography` (`crates/quarto-core/src/project/book/bibliography.rs`) seeds citation numbers in book-wide first-cited order. Sorting by number gives the right order there too.

### Reproduction (fixtures in `2026-11-07-citeproc-unsorted-bibliography-order-investigation/`)

Pandoc 3.12.1 output for each fixture is in `pandoc-3.12.1-expected.txt` (`pandoc X.qmd -f markdown --citeproc -t plain`). At HEAD, q2 renders the same numbers in a different order:

| Fixture | Pandoc order | q2 order (HEAD) |
|---|---|---|
| `cited-order-reversed.qmd` (cites `second`, then `cited`) | `[1]` Adams, `[2]` Smith | `[2]` Smith, `[1]` Adams |
| `nocite-order.qmd` (`nocite: @uncited, @extra`) | `[1]` Adams, `[2]` Smith, `[3]` Doe, `[4]` Brown | `[3]` Doe, `[2]` Smith, `[1]` Adams, `[4]` Brown |
| `nocite-order-reversed.qmd` (`nocite: @extra, @uncited`) | same as above: uncited entries follow **file order, not `nocite` order** | as above |
| `nocite-star.qmd` (`nocite: @*`, cites `second`) | `[1]` Adams, then Doe, Smith, Brown in file order | file order |

So the rule to match: cited entries in citation-number order, then the uncited entries in load order. Because pampa numbers the uncited entries in load order, this is the same as sorting by citation number.

### CSL conformance suite

The harness (`tests/integration/csl_conformance.rs`) cites every input item in input order unless a test supplies `CITATIONS` or `CITATION-ITEMS`. A script over `test-data/csl-suite/` found 15 bibliography-mode tests that supply explicit citations and whose style has no bibliography `<sort>`. Eight are enabled and seven disabled. In all 15, first-cited order equals input order, so **no conformance test can tell the two orderings apart**. The change should not move the suite in either direction; Phase 0 confirms this by running the full report before and after.

## Decisions (2026-11-07)

1. The fix lives in `quarto-citeproc`, in the no-sort branch, so every caller gets Pandoc's order.
2. Merge the two generators: `generate_bibliography` renders the entries from `generate_bibliography_to_outputs`. Side effect: the string API now applies `subsequent-author-substitute`, which only the AST path applied before. Only tests call the string API.
3. References without a citation number go after the numbered ones, in insertion order, which matches Pandoc.

## Checklist

### Phase 0: tests first

- [x] `quarto-citeproc` unit test: no-sort style, citation order differs from insertion order, so the bibliography follows citation numbers.
- [x] `quarto-citeproc` unit test: an unnumbered reference comes after the numbered ones, in insertion order.
- [x] `quarto-citeproc` unit test: the string API applies `subsequent-author-substitute`, as the AST API does.
- [x] pampa filter tests: cited order reversed; `nocite` order reversed (uncited entries follow load order).
- [x] Run them and confirm they fail.
- [x] Baseline the CSL conformance suite (`cargo nextest run -p quarto-citeproc`).

### Phase 1: implementation

- [x] The no-sort branch orders ids by initial citation number; unnumbered ids follow in insertion order.
- [x] `generate_bibliography` delegates to `generate_bibliography_to_outputs`.

### Phase 2: verification

- [x] `cargo nextest run -p quarto-citeproc -p pampa -p quarto-core -p quarto`; clippy for the touched crates.
- [x] End-to-end `q2 render` of the investigation fixtures, compared with `pandoc-3.12.1-expected.txt`.
- [x] Rerun the conformance suite and confirm the enabled set still passes.

### Result

End to end, `q2 render` of the four investigation fixtures produces the same entries, numbers, and order as Pandoc 3.12.1. The output is saved as `q2-after-fix.txt` next to `pandoc-3.12.1-expected.txt`. The only remaining difference is punctuation placement in quotes, which bd-citeproc-cluster-collapse-kns8uggw tracks. No snapshot files changed. I did not run a separate numeric-style book smoke test; the existing `quarto-core` book tests pass.

## Incidental finding (not in scope)

In every fixture, Pandoc prints `“Cited work,” Journal` (en-US puts punctuation inside quotes), and q2 prints `“Cited work”, Journal`. `quarto-citeproc` has `move_punctuation_inside_quotes` and `Processor::punctuation_in_quote()`, but only the conformance harness and `csl_conformance_report` call them; pampa's citeproc filter does not. bd-citeproc-cluster-collapse-kns8uggw already tracks this ("bibliography punctuation outside quotes"). I linked it as related and commented there with this evidence instead of filing a duplicate.

## Risks / tradeoffs (draft)

- Low risk to the suite (see above). The fixtures that pampa, quarto-core, and the smoke tests share could still change order where a document cites references out of file order with a no-sort style. Any such snapshot change has to be reported explicitly.
- Book merge: the order should only become more correct. A numeric-style book smoke test in Phase 2 checks this.
