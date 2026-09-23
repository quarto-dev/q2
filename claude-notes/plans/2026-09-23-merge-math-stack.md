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

- [ ] **#705** — merge main, regenerate the two artifacts, tests, push,
      mark ready, merge when CI is green.
- [ ] **#706** — retarget to main, merge main, union-resolve catalog +
      sidebar, refresh `Cargo.lock`, build + tests + lint, push, merge.
- [ ] **#708** — merge main, take #710's `crossref_render.rs`, fix the
      module list, build + tests, push, merge.
- [ ] **#709** — retarget to main, merge main, resolve plan doc, tests,
      push, merge.
- [ ] **#710** — retarget to main, merge main; gate `MathMlStage` on
      `is_html_based()`, add `math-ml` to `PANDOC_STAGE_EXCLUDED`, update
      `t6_2_pandoc_stage_list_produces_exact_surviving_name_list`, add an
      end-to-end test (mathml-method document `--to docx` keeps its OMML
      equation); build + tests; push; merge.

## Verification log

(filled in as each PR lands)
