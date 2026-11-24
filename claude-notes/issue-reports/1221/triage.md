# Issue #1221 — HTML bibliography has no "References" heading; `reference-section-title` ignored

- **GitHub**: https://github.com/quarto-dev/q2/issues/1221
- **Reporter**: @rich-iannone (Richard Iannone), 2026-11-16
- **Triage date**: 2026-11-24
- **Branch**: `issue-1221` (topic branch in the main checkout at the user's request, not a worktree; based on `main` @ `5eb36805b`)
- **Braid strand**: bd-7mceevmx
- **Scope**: HTML output only (the issue's two asks: bibliography moved into
  `#quarto-appendix` under a localized "References" heading, and
  `reference-section-title` overriding that heading). The Typst side is
  #1105 and is excluded.

## Summary

Reproduced on `main`. In a plain HTML document with `bibliography:`, the
`#refs` div ends up as a bare top-level block after the body. It gets no
`#quarto-appendix` wrapper, no `#quarto-bibliography` section and no heading.
The code that builds that structure (`wrap_bibliography` in
`transforms/appendix.rs`) exists and already localizes the heading. It never
runs because of **pipeline ordering**: for non-book HTML, citeproc is placed
in the *post* user-filter bucket, and that bucket runs after
`AstTransformsStage`, which contains `AppendixStructureTransform`. So when the
appendix transform runs, no `#refs` div exists yet. Separately,
`reference-section-title` is not read anywhere in `crates/`. This is a real
bug. The fix is moderate in size: it changes ordering, and the
`reference-section-title` tier is small.

## Reproduction

Fixtures are in this directory: `refs.bib`, `repro-en.qmd`, `repro-fr.qmd`,
`repro-titled.qmd` (the three pages from the issue), plus two experiments:

- `exp-citeproc-pre.qmd` uses `filters: [citeproc, quarto]`, which forces
  citeproc into the *pre* bucket.
- `exp-with-footnote.qmd` is a citation plus a footnote, so an appendix exists
  for other reasons.

`claude-notes/` is itself a Quarto project that excludes these files, so
render copies of them outside the repo:

```sh
S=$(mktemp -d); cp claude-notes/issue-reports/1221/* "$S"; cd "$S"
for p in repro-en repro-fr repro-titled exp-citeproc-pre exp-with-footnote; do
  /path/to/q2/target/debug/q2 render $p.qmd
done
```

The `<main>` element of each output is saved under `output/*.main.html`.

| File | `#quarto-appendix` | Bibliography heading | `#refs` location |
|---|---|---|---|
| repro-en | none | none | top level, right after the body |
| repro-fr | none | none (expected "Les références") | top level |
| repro-titled | none | none (expected "Sources") | top level |
| exp-citeproc-pre | yes | **"References"** in `section#quarto-bibliography` | inside the appendix ✔ |
| exp-with-footnote | yes (Footnotes only) | none | **after** the closing `#quarto-appendix` |

`exp-citeproc-pre` shows that once citeproc runs before the AST transforms, the
existing appendix code produces the expected structure. `exp-with-footnote`
shows the ordering bug in its worst form: the bibliography lands *below* the
appendix, after the footnotes.

Expected output, from Quarto 1.9.37 as quoted in the issue:
`<div id="quarto-appendix" class="default"><section class="quarto-appendix-contents" role="doc-bibliography" id="quarto-bibliography"><h2 class="anchored quarto-appendix-heading">References</h2><div id="refs" class="references csl-bib-body hanging-indent" role="list">`

## Localization

- `crates/quarto-core/src/pipeline.rs:507-509`: `UserFiltersStage::pre()` →
  `AstTransformsStage` → `UserFiltersStage::post()`.
  `AppendixStructureTransform` is in the Finalization phase of
  `AstTransformsStage` (`pipeline.rs:2883`).
- `crates/quarto-core/src/stage/stages/user_filters.rs:125-143`
  (`citeproc_defaults`): for non-hybrid (HTML) formats,
  `before_quarto: in_book`, so a plain HTML document gets citeproc *after*
  the `quarto` sentinel, in the post bucket.
  `filter_resolve.rs:122-140` documents the choice: it mirrors Q1, which
  runs citeproc after crossref.
- `crates/pampa/src/citeproc_filter.rs:1664-1700`: fills an existing
  `#refs` (searching nested Divs) or appends a new top-level `#refs` div at the
  end of the document. This is how it lands after `#quarto-appendix`.
- `crates/quarto-core/src/transforms/appendix.rs:141-147`: the comment
  "CiteprocTransform will create this later" is stale (no such transform
  exists, and citeproc is a user-filter-stage filter). `extract_bibliography`
  only scans top-level blocks.
- `appendix.rs` `wrap_bibliography` / `appendix_heading` / `appendix_title`:
  the heading is localized via `section-title-references`. `appendix_title`'s
  doc comment states that no per-document override tier exists ("Quarto 1
  exposes no `footnotes-title`-style option"). That holds for footnotes, but
  **not** for the bibliography: `reference-section-title` is that option.
- Why Q1 works: Q1's appendix consolidation is an HTML DOM postprocessor that
  runs after Pandoc (and so after citeproc). Order inside Pandoc does not matter
  there.

## Open questions — resolved during triage

1. *Is the appendix transform itself broken, or only starved of input?*
   It is only starved of input. `exp-citeproc-pre` produces the right
   section and heading with no code change.
2. *Is `reference-section-title` read anywhere?* No:
   `grep -rn "reference-section-title\|reference_section_title" crates/ --include='*.rs'`
   returns nothing.
3. *Does this affect books?* Books are a different path: book chapters
   get citeproc in the pre bucket (`before_quarto: in_book`), and
   `AppendixStructureTransform` skips `book: true` anyway. They are out of
   scope here.

## Outcome / recommended next step

Real bug. Filed bd-7mceevmx. The fix
needs one of these approaches, chosen at implementation time:

- **(A, preferred) Late bibliography placement.** Keep citeproc in the post
  bucket, which preserves Q1's citeproc-after-user-filters/crossref ordering.
  Add a small step after `UserFiltersStage::post()` for HTML that finds `#refs`
  (top level, or citeproc's appended div), wraps it with `wrap_bibliography`,
  and inserts it into `#quarto-appendix`, creating that container if absent.
  Q1 puts the bibliography *before* footnotes. Honor `appendix-style: none`,
  `book`, and `citation-location: margin` exactly as the transform does today,
  and remove the bibliography branch from `AppendixStructureTransform` (or
  share its helpers).
- (B) Move citeproc into the pre bucket for HTML. This is smaller but changes
  citeproc's order relative to crossref and user filters, which Q1 deliberately
  doesn't do. Not recommended.

Plus, in either case:

- Add a `reference-section-title` tier to the bibliography heading. Precedence:
  `reference-section-title` → `section-title-references` language term →
  "References". Update the `appendix_title` doc comment accordingly.
- Fix the stale "CiteprocTransform will create this later" comment.
- Tests: the three repro pages (en / fr / titled) plus the
  citation-plus-footnote case (bibliography inside the appendix, before the
  footnotes). Pick tests per `crates/quarto-core` conventions.

Noted but out of scope (follow-ups if wanted): Q1 emits
`class="quarto-appendix-contents"` on the section, `hanging-indent` on `#refs`
(style-dependent, from the CSL `hanging-indent` option), and `role="list"`.
q2 emits `class="section"` and none of the extras. A user-placed nested
`::: {#refs}` is not moved, because `extract_bibliography` is top-level only.
Decide during the fix whether Q1 moves it.

## Verification commands used

```sh
gh issue view 1221 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
cargo xtask verify --skip-hub-build   # Rust: all green. ts-packages build failed on
                                      # missing @viz-js/viz (stale node_modules);
                                      # `npm install` then re-ran the remaining steps.
grep -rn "reference-section-title" crates/ --include='*.rs'
target/debug/q2 render <each fixture>.qmd   # in a scratch copy, see Reproduction
```

## Cross-references

- GH #1105: Typst `#refs` placement and a stray translated heading in the appendix
- GH #1026: citeproc enabled by default from `bibliography:` (`apply_citeproc_default`)
- GH #1225: footnote citations with citeproc in the pre bucket
- `crates/quarto-core/src/transforms/appendix.rs` module docs

## Fix (same branch)

Approach (A), implemented on `issue-1221`:

- `transforms/appendix.rs`: new `place_bibliography` moves a top-level
  `#refs` into `#quarto-appendix`. It inserts the section before the
  footnotes and metadata sections, or creates the container if there is none.
  It uses the transform's gates. The bibliography heading reads
  `reference-section-title` first (`bibliography_title`). The bibliography gate
  is now `citation-location: margin`, as in Q1's `hasMarginCites`, instead of
  `reference-location`.
- `stage/stages/appendix_bibliography.rs`: `AppendixBibliographyStage` calls
  it right after `UserFiltersStage::post`. It runs in HTML and q2-preview and
  is excluded from the Pandoc-writer formats (`PANDOC_STAGE_EXCLUDED`).
- Q1 doesn't move a `#refs` the author placed under their own heading,
  because the div is nested in that heading's section. The fix keeps that
  behavior, and `html_citations::author_placed_refs_under_heading_stay_in_place`
  pins it.
- Tests: `tests/integration/html_citations.rs` covers the three issue pages,
  footnote ordering, the margin gates and `appendix-style: none`. Unit tests
  for `place_bibliography` are in `appendix.rs`.
