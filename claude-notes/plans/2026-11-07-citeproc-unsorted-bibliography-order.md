---
title: 'citeproc: unsorted styles order the bibliography by file order (bd-mumobar8)'
date: 2026-11-07
description: 'For CSL styles without a bibliography sort, list entries by citation number, as Pandoc does, instead of bibliography-file order.'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-mumobar8
---

**Branch:** `braid/bd-mumobar8-citeproc-unsorted-styles-order` (topic branch in the main checkout, based on `main` @ `e95a3efbf`; no PR, per the user)
**Do not start implementation until the user gives the go-ahead.**

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

## Proposed phases (draft)

- Phase 0: tests first.
  - Unit tests in `quarto-citeproc`: a no-sort style where citation order differs from insertion order, and an uncited reference that has no number.
  - Pampa filter tests covering the `cited-order-reversed` and `nocite-order-reversed` cases.
  - A baseline from `csl_conformance_report`.
- Phase 1: in the no-sort branch, order ids by initial citation number. Ids with no number come after, in insertion order. Possibly merge the two generator functions first (design question 2).
- Phase 2: end-to-end `q2 render` of the fixtures, compared against `pandoc-3.12.1-expected.txt`. Book smoke test with a numeric style.
- Phase 3: rerun the conformance report and confirm the enabled set is unchanged.

## Open design questions for the user

1. **Where should the fix live?** Option (a), recommended: in `quarto-citeproc`\'s no-sort branch, since the `Processor` already owns the citation numbers and every caller (single documents, book merge, Lua `pandoc.utils.citeproc` once k-476 lands) would get Pandoc's order. Option (b): reorder in pampa after generation, which leaves the crate's library behavior as file order.
2. **Merge the duplicate generators?** `generate_bibliography` (string output) and `generate_bibliography_to_outputs` (Output AST) repeat the same sort and renumber logic. Should this change merge them, for example with the string version calling the AST version, or should it touch both and leave the merge for later?
3. **References with no citation number.** These only happen for library callers that never number uncited references (pampa now always does). Is "after the numbered ones, in insertion order" the right fallback? It matches Pandoc's observable result.

## Incidental finding (not in scope)

In every fixture, Pandoc prints `“Cited work,” Journal` (en-US puts punctuation inside quotes), and q2 prints `“Cited work”, Journal`. `quarto-citeproc` has `move_punctuation_inside_quotes` and `Processor::punctuation_in_quote()`, but only the conformance harness and `csl_conformance_report` call them; pampa's citeproc filter does not. bd-citeproc-cluster-collapse-kns8uggw already tracks this ("bibliography punctuation outside quotes"). I linked it as related and commented there with this evidence instead of filing a duplicate.

## Risks / tradeoffs (draft)

- Low risk to the suite (see above). The fixtures that pampa, quarto-core, and the smoke tests share could still change order where a document cites references out of file order with a no-sort style. Any such snapshot change has to be reported explicitly.
- Book merge: the order should only become more correct. A numeric-style book smoke test in Phase 2 checks this.
