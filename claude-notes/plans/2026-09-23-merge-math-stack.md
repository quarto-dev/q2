# Merging the math stack (PRs 705, 706, 708, 709, 710) onto main after #704

## Overview

PR #704 (Pandoc-hybrid docx/pptx/epub/typst) landed on main on 2026-09-21.
The math stack — `Math.text_source` (#705), `quarto-math` (#706), the
equation-numbering stage (#708), the MathML writer (#709) and the
`MathMlStage` (#710) — was branched before it. This plan records the
conflict assessment (2026-09-23) and tracks the merge, one PR at a time.

Parent plans: `2026-09-21-quarto-math-and-native-docx.md`,
`2026-09-21-equation-numbering-and-mathml.md`.

## Assessment (2026-09-23, trial merges against origin/main 6d648d92)

- **#705**: conflicts only in two regenerable artifacts
  (`crates/pampa/snapshots/json/math-with-attr.snap`,
  `ts-packages/annotated-qmd/examples/academic-paper.json`). Rust
  auto-merges and compiles; pampa/quarto-core/pandoc-types tests pass after
  regeneration. Pandoc 3.11 ignores the new `textS` sidecar on the
  Pandoc leg. docx/typst output of a labelled equation unchanged.
- **#706**: `Cargo.lock` (both sides add packages; refresh from main),
  `error_catalog.json` (main added Q-20/Q-21, 706 adds Q-22; union),
  `docs/_quarto.yml` (errors sidebar: `pandoc`/`typst` vs `math` sections;
  union). Lint passes on the union.
- **#708**: `tests/integration/main.rs` module list; and once #705 is on
  main, `crossref_render.rs` (705 extended `text_source` in
  `render_equation`, 708 rewrote it). #710 already carries the
  resolution (`append_to_tex` in `equation_number.rs`) — take 710's
  `crossref_render.rs`. docx/typst equation numbers are unaffected:
  pandoc drops q2's `\tag{N}` anyway, and the number comes from the
  vendored Quarto 1 `crossref/equations.lua` (`\qquad(N)`). Verified by
  rendering before/after with the real binary.
- **#709**: plan-doc checkbox conflict only.
- **#710**: one real regression against #704 — `MathMlStage` gates on
  `html-math-method` only, not on the format, and is not on
  `PANDOC_STAGE_EXCLUDED`. A document with `html-math-method: mathml`
  rendered `--to docx` converts its math to `RawInline` before the
  Pandoc leg, and the vendored `equations.lua` crashes
  (`attempt to concatenate a nil value (field 'text')`, pandoc exit 83,
  Q-20-3). Decision (user, 2026-09-23): **ignore the option silently on
  non-HTML formats**, matching `EquationNumberStage`'s `Writer` no-op and
  Quarto 1 (which forwards the key to pandoc, whose non-HTML writers
  ignore it). No warning: the `html-` prefix is what makes the key safe
  in shared metadata for multi-format projects.

## Merge order and checklist

- [x] **#705** — merged main (`cb0fccde`, artifacts regenerated), 9,803
      pampa/quarto-core/pandoc-types tests green locally, CI green,
      merged 2026-09-23 (`5fcafab3`).
- [x] **#706** — merged the #705 tip (`c84dfcee`: catalog + sidebar
      union, `Cargo.lock` refreshed from main), workspace build + lint +
      9,723 tests green locally, CI green, merged 2026-09-23 (`e276f50c`).
- [x] **#708** — merged the #705 tip taking #710's `crossref_render.rs`
      (`1999946d`) and lifted `append_to_tex` into `equation_number.rs`;
      the Pandoc exact stage-list assertion gains `equation-number`
      (`d0cf22d6`). Clippy + 9,645 tests green locally, CI green, merged
      2026-09-23 (`2bcd922a`).
- [x] **#709** — GitHub's stack auto-rebased the branch onto main after
      #706; merged main again after #708 (`cb8132ee`, plan doc from the
      Phase 3 branch). Merged after CI.
- [x] **#710 → re-opened as #714** — the stack refused a base change and
      would have rebased this branch's merge history when #709 landed, so
      the same branch was re-opened against `main`. Carries the
      `MathMlStage` fix (`56b93af7`: `applies_to` gate, `math-ml` on
      `PANDOC_STAGE_EXCLUDED`, docx end-to-end test) — the new tests were
      confirmed failing without the fix, then 9,777 quarto-core /
      quarto-math / pampa tests green with it. Merged after CI.

## Verification log

End-to-end through the real binary (`cargo run --bin q2 -- render`), at
the #714 tip, fixture: a labelled display equation `$$E = mc^2$$ {#eq-e}`
with `See @eq-e.`:

- `--to docx` with `html-math-method: mathml` in front matter: renders;
  `word/document.xml` has one `<m:oMath>` reading `E=mc2  (1)` (number
  from the vendored `crossref/equations.lua`), no MathML in the body.
  Before the fix this invocation failed with Q-20-3 / pandoc exit 83.
- `--to typst`: PDF text shows the equation and `(1)`, unchanged from
  main.
- `--to html` with `mathml`: `<math xmlns=… display="block">` emitted,
  the sibling `<span class="quarto-eq-number">(1)</span>` present, zero
  references to MathJax in the page.
- `--to docx` without the option: unchanged from main (`E=mc2  (1)`).

Known non-blocker seen while checking output: the MathML writer nests
`E = mc^2` as `<msup><mrow>E=mc</mrow><mn>2</mn></msup>` (superscript base
is the whole run). Visually identical, semantically off; that is the mitex
base-selection quirk already filed as bd-0mzhnxft.
