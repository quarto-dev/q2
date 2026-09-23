# Format-specific equation numbering and `html-math-method: mathml`

**Status:** approved 2026-09-21 (all five open decisions settled with the
user, each as recommended). Phases 1–3 implemented the same day. PRs: #708 (Phase 1,
against `main`), #709 (Phase 2, stacked on #706) and #710 (Phase 3, stacked
on #709), the last two linked into the GitHub stack #705 → #706 → #709 → #710.
**Strands:** bd-vlhi2zkj (Phase 1, equation numbering; branch
`braid/bd-vlhi2zkj-equation-numbering` off `main`), bd-9z83tcv0 (Phase 2,
MathML writer, p2), bd-3evfzwal (Phase 3, `MathMlStage`; blocked on the
other two).
**Parent work:** `claude-notes/plans/2026-09-21-quarto-math-and-native-docx.md`
(quarto-math, PR #706). This plan is the "interlude": it gives quarto-math a
real consumer (`format: html`) before the docx writer exists.

## Overview

Two things, in dependency order.

1. **Equation numbering becomes a format-specific presentation step.**
   Today `render_equation` (`crates/quarto-core/src/transforms/crossref_render.rs`)
   appends `\tag{N}` to the TeX of every numbered equation, unconditionally,
   inside the format-agnostic `CrossrefRenderTransform`. `\tag` is an
   `amsmath` command that only MathJax and KaTeX understand: Quarto 1
   (`crossref/equations.lua`, `renderEquation`) emits it *only* for HTML with
   `mathjax`/`katex`, uses `\qquad(N)` for every other math engine, and
   defers to the writer for LaTeX (`equation` + `\label`) and Typst
   (`#math.equation(numbering:)`). Pandoc's converter rejects `\tag` in
   `$$…$$` (verified with pandoc 3.9: warning + verbatim fallback under
   `--mathml` and `-t docx`), and quarto-math's spec marks it unsupported
   (verified: an `Error` node, output withheld). The number encoding is
   presentation, so it moves out of crossref-render into a step selected by
   the format and `html-math-method`.

   The hand-off between the two is a **reserved attribute on the equation
   `Span`**. Crossref-render leaves the `Math` node byte-identical to the
   source and records the number as `quarto-eq-number="N"`; the numbering
   step consumes and removes it. Because the step runs *after* user post
   filters, a Lua filter can read, rewrite or drop the attribute — the
   escape hatch — without knowing anything else about the pipeline.

2. **`html-math-method: mathml`** renders each `Inline::Math` to MathML Core
   at render time through a new `quarto_math` writer, so common documents
   ship no MathJax. Today the option is a silent no-op (verified on the PR
   #706 tip: raw `\(…\)` TeX, no renderer loaded). With (1) in place the
   MathML path needs no `\tag` support: the number is a sibling inline
   outside `<math>`, which MathML Core requires anyway (`mlabeledtr` was
   dropped from Core).

### Why the numbering step is a *stage*, not a Finalization transform

The HTML stage list (`build_html_pipeline_stages_with_options`,
`crates/quarto-core/src/pipeline.rs`) is

```
… → UserFiltersStage::pre → AstTransformsStage (all four phases, incl.
CrossrefRenderTransform) → UserFiltersStage::post → ResourceReportStage →
CodeHighlightStage → MathJsStage → RenderHtmlBodyStage → ApplyTemplateStage
```

Every Finalization transform runs inside `AstTransformsStage`, *before* post
filters. A numbering transform there would consume the reserved attribute
before any user filter could see it. Placing the step as a stage between
`UserFiltersStage::post` and `MathJsStage` (the slot `CodeHighlightStage`
already occupies: AST-level, format-aware, post-filter) keeps the attribute
visible to post filters and still satisfies the phase contract's intent
(presentation after crossref-render). The design doc gets a paragraph naming
this slot (item in Phase 1).

### The reserved attribute

- Key: `quarto-eq-number` (kv on the `Span#eq-… .quarto-math-with-attribute`
  that `render_equation` already emits). `quarto-` prefix matches the other
  reserved names transforms consume (`quarto-template-params`,
  `quarto-reuse`, `quarto-xref`).
- Value: the number as text, exactly what would be typeset (`1`, later
  `2.3` when section-prefixed numbering lands; that is out of scope here).
- Lifetime: written by `CrossrefRenderTransform`, readable by post filters,
  removed by `EquationNumberStage` in every strategy (so it never reaches the
  writer, in any format).
- Lua view: `el.attributes["quarto-eq-number"]` on a `Span` whose
  `identifier` starts with `eq-`. Deleting the attribute suppresses the
  number; changing it changes the label. Documented for filter authors.

### Numbering strategies

Selected by `EquationNumberStage` from `ctx.format` and the document's
`html-math-method` (already flattened to top level by `resolve_format_config`;
parsed by a shared `MathMethod` enum, see Phase 1):

| Strategy | When | Effect on `[Math(Display, t)]` |
| --- | --- | --- |
| `TexTag` | HTML-based format, method `mathjax` / `katex` / absent | `t` becomes `t\tag{N}` (today's behaviour, with the same `text_source` concat provenance) |
| `Qquad` | HTML-based, method `plain` / `webtex` / `gladtex` / unknown | `t` becomes `t \qquad(N)` (Quarto 1's non-JS encoding) |
| `Sibling` | HTML-based, method `mathml` | `Math` untouched; a `Span.quarto-eq-number` containing `Str("(N)")` is appended after it inside the equation span, and the equation span gains class `quarto-eq-sibling-number` for CSS |
| `Writer` | LaTeX / Typst / docx (native writers, future) | `Math` untouched; the writer numbers. Placeholder: q2 renders only HTML and revealjs natively today, and PR #704 routes docx/Typst through Quarto 1's Lua filters, which do their own numbering |

`Sibling` is robust to a failed MathML conversion: if the expression falls
back to TeX + MathJax (Phase 3 hybrid), the number is still there, outside
the math.

### MathML output shape

```html
<span class="math display">
  <math xmlns="http://www.w3.org/1998/Math/MathML" display="block">
    <semantics>
      <mrow>…</mrow>
      <annotation encoding="application/x-tex">ORIGINAL TEX</annotation>
    </semantics>
  </math>
</span>
```

The existing `span.math.inline` / `span.math.display` wrappers stay (user
CSS and the preview-parity tooling key on them; Pandoc drops the span for
display math, we do not). The annotation carries the source TeX for
copy/paste and assistive tools. Emitted as `RawInline("html", …)` replacing
the `Inline::Math`, which the HTML writer passes through verbatim.

## Checklist

### Phase 0 — setup

- [x] (done 2026-09-21: bd-vlhi2zkj, bd-3evfzwal; bd-9z83tcv0 → p2) File the two new strands (numbering restructure; HTML `mathml` stage)
      with `discovered-from: bd-entbg6x3`, `related: bd-9z83tcv0`; link this
      plan. Reprioritise bd-9z83tcv0 from p3 to p2.
- [x] (branch created 2026-09-21) Phase 1 branches off `main` (it does not touch quarto-math) and lands
      as its own PR, same pattern as decision 5 of the parent plan. Phases 2
      and 3 branch off `feature/bd-entbg6x3-quarto-math` and stack on #706;
      Phase 3 merges `main` once Phase 1 is in.

### Phase 1 — equation numbering as a format-specific stage (new strand, PR against `main`)

Tests first, in this order.

- [x] (2026-09-21) **Unit tests in `crossref_render.rs`**: `render_equation` leaves
      `math.text` and `math.text_source` untouched and sets
      `quarto-eq-number` on the span; an unnumbered equation sets no
      attribute. Rewrite the three existing tests that assert `\tag{1}` in
      the math text (`…:2494`, `:2536`, `:2595`) to assert the attribute.
- [x] (2026-09-21; 13 tests in `equation_number.rs`) **`EquationNumberStage` unit tests** (one per strategy): `TexTag`
      appends `\tag{N}` and extends `text_source` as a concat with a
      synthesized piece (move the provenance code from `render_equation`);
      `Qquad` appends ` \qquad(N)`; `Sibling` appends the label span and the
      modifier class; every strategy removes the attribute; a span without
      the attribute is untouched; an equation span whose first inline is not
      `Math(DisplayMath)` is left alone with a debug trace (mirrors the
      preview's `Equation.tsx` fallback rules).
- [x] (2026-09-21; `math_method.rs` + `NumberEncoding::for_document` tests) **Strategy selection tests**: `MathMethod::from_meta` for the string
      and object forms (`mathjax`, `katex`, `mathml`, `plain`, `webtex`,
      `gladtex`, unknown, absent); `strategy_for(format, method)` table.
- [x] (2026-09-21; new file `tests/integration/equation_numbering_pipeline.rs`, 13 tests, so the math-js suite stays about engine injection) **End-to-end tests**
      (drive `render_to_file`): default → `\tag{1}` present and MathJax
      loaded (the existing `labelled_equation_emits_mathjax_and_tag` keeps
      passing); `html-math-method: plain` → `\qquad(1)` present, no
      `\tag`, no loader; `html-math-method: mathml` → `span.quarto-eq-number`
      present, `\tag` absent (the math itself stays TeX until Phase 3);
      revealjs → `\tag{1}`.
- [x] (2026-09-21; same file; also pins that a *pre* filter does not see the attribute) **Lua escape-hatch test**: a post filter that reads
      `el.attributes["quarto-eq-number"]` and rewrites it to `A` yields
      `\tag{A}` in the output; a filter that deletes it yields an unnumbered
      equation. Place next to the existing user-filter integration tests.
- [x] (2026-09-21) Implement: `MathMethod` enum in a new
      `crates/quarto-core/src/math_method.rs` (string + object forms; the
      only parser of `html-math-method`), and make `MathEngine::from_meta`
      in `math_js.rs` a thin mapping over it so the two stages cannot drift.
- [x] (2026-09-21; `EQ_NUMBER_ATTR` lives in `crossref/mod.rs` next to the `EQUATION` type name) Implement: `render_equation` writes the attribute instead of the tag.
- [x] (2026-09-21; registered between `ResourceReportStage` and `CodeHighlightStage`) Implement: `crates/quarto-core/src/stage/stages/equation_number.rs`,
      registered between `UserFiltersStage::post` and `CodeHighlightStage`
      in `build_html_pipeline_stages_with_options`. Included in the
      q2-preview stage list (it is a no-op there: the preview excludes
      crossref-render, so no span carries the attribute, and `Equation.tsx`
      keeps its own KaTeX `\tag` append). Add the stage name to the
      preview-exclusion validator's known list if the test requires it.
- [x] (2026-09-21; **placement changed from the plan**: not `_bootstrap-rules.scss` but a shared layer `resources/scss/html/templates/equation-number.scss`, loaded by `load_equation_number_layer` in `quarto-sass` at the five HTML compile sites and in `assemble_reveal_scss`, exactly like `copy-code.scss`. Reason: revealjs is HTML-based and gets the `Sibling` encoding too, and the bootstrap rules file is not bundled into decks. Tests: `test_compile_default_css`, `test_compile_reveal_theme_includes_equation_number_rules`.) CSS for `Sibling`: `.quarto-eq-sibling-number`
      as a flex row with the math centered and the label pushed right.
- [x] (2026-09-21; new section "The post-filter presentation slot") Design doc: add the "post-filter presentation stage" slot to
      `claude-notes/designs/transform-pipeline-phases.md`, naming
      `CodeHighlightStage` and `EquationNumberStage` as its members and
      stating the rule: a step that must remain visible to user post filters
      is a stage here, not a Finalization transform.
- [x] (2026-09-21; placed in `docs/guides/authoring/lua-filters.qmd` as its own section, since that is where filter authors look; the cross-reference page in that directory is misnamed `cross-references.cmd`, flagged to the user) Docs (user-facing):
      one short "for filter authors" note on `quarto-eq-number`.
- [x] (2026-09-21) Update the `Equation.tsx` comment that cites `render_equation`'s
      line numbers and the `\tag` port, so the two stay traceable.
- [x] (2026-09-21: full `cargo xtask verify` green — 14049 Rust tests, ts-packages, hub-client build + tests. Two collateral test updates: the HTML stage-list/count assertions in `pipeline.rs` (25 → 26 stages) and the `styles.css` byte-identity baseline in `tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt`, re-captured with a dated note because the new SCSS layer is additive to every compiled stylesheet.) `cargo xtask verify` (full: `quarto-core` changed). Record the
      end-to-end invocation and the inspected output here.

**End-to-end verification (2026-09-21, real binary, output inspected).**
Three copies of the same document, differing only in front matter
(`html-math-method` absent / `mathml` / `plain`), each with
`$$\sum_{i=1}^{n} \alpha_i = \int_0^\infty e^{-x}\,dx$$ {#eq-one}` and a
`@eq-one` reference, rendered with
`cargo run --bin q2 -- render <scratch>/{default,mathml,plain}.qmd`. The
equation markup in each `.html`:

```html
<!-- default (MathJax loaded; window.MathJax present once) -->
<span id="eq-one" class="quarto-math-with-attribute"><span class="math display">\[
\sum_{i=1}^{n} \alpha_i = \int_0^\infty e^{-x}\,dx
\tag{1}\]</span></span>
<!-- html-math-method: mathml (no engine loaded) -->
<span id="eq-one" class="quarto-math-with-attribute quarto-eq-sibling-number"><span class="math display">\[
\sum_{i=1}^{n} \alpha_i = \int_0^\infty e^{-x}\,dx
\]</span><span class="quarto-eq-number">(1)</span></span>
<!-- html-math-method: plain (no engine loaded) -->
<span id="eq-one" class="quarto-math-with-attribute"><span class="math display">\[
\sum_{i=1}^{n} \alpha_i = \int_0^\infty e^{-x}\,dx
 \qquad(1)\]</span></span>
```

`See <a href="#eq-one" class="quarto-xref">Equation 1</a>.` in all three;
`quarto-eq-number="…"` appears in none. `<stem>_files/styles.css` of each
contains the three `.quarto-eq-sibling-number…` rules from
`equation-number.scss`.

**Follow-up for the #706 rebase.** On `main` today `Math` has no
`text_source`; PR #705 adds it and PR #706's `render_equation` extends the
mapping with a synthesized piece for the appended `\tag`. After this phase
merges, that provenance code belongs in `EquationNumberStage::encode_number`
(the `TexTag` and `Qquad` arms are the only places that append to the
text). Noted in the parent plan's merge-readiness section on the #706
branch when it rebases.

### Phase 2 — MathML writer in quarto-math (bd-9z83tcv0, stacked on #706)

Mirrors `typst.rs`: one file, one snapshot per fixture, a corpus-wide
validity check standing in for the compile check Typst has.

- [x] (2026-09-21; `every_fixture_is_valid_mathml_core`, a quick-xml walk checking element allowlist, fixed child counts for `mfrac`/`msub`/…/`munderover`, and `mathvariant`) **Validity test** (`tests/integration/mathml.rs`): every fixture's
      output parses with `quick-xml`, uses only MathML Core elements
      (`math mrow mi mn mo mtext mspace ms msub msup msubsup munder mover
      munderover mfrac msqrt mroot mtable mtr mtd mstyle mpadded mphantom
      merror semantics annotation`, plus `menclose`, see decisions), and
      the only `mathvariant` value is `normal`. Snapshot per fixture.
- [x] (2026-09-21; 13 structural tests) **Structural tests**: `Run` splits to `mi`/`mn`/`mo` via
      `split::split_run` with a single-letter `mi` italic by default;
      `Nary` + `Scripts` → `munderover` in display / `msubsup` inline
      (`LimLoc` rules identical to OMML); `Func` → `mi` + U+2061 function
      application; `Delimited` → `mrow` with stretchy fence `mo`s and
      `\middle` separators; `Frac` styles (`NoBar` → `linethickness="0"`,
      `Binom` wrapped in parentheses, `Display`/`Text` → `mstyle
      displaystyle`); `Sqrt` → `msqrt`/`mroot`; `Accent` → `mover
      accent="true"`; `Bar`/`GroupChr`/`LimPos`/`XArrow` → `munder`/`mover`;
      `Matrix` layouts → `mtable` (`Cases` with a left `{` fence and
      `columnalign="left"`, `Aligned` with alternating right/left
      alignment, `Gathered` centered, `Matrix` inside its fences);
      `Break` → the whole row becomes a two-row `mtable`; `Space` →
      `mspace width="…em"` (negative widths allowed in Core);
      `Phantom` → `mphantom` (+ `mpadded` for `h`/`v` only);
      `Color` → `mstyle mathcolor`; `Text` → `mtext`.
- [x] (2026-09-21; `styled_char` + unit test over every variant and hole) **Style variants**: `Style { variant }` maps each letter and digit of
      its body into the Mathematical Alphanumeric Symbols block
      (`U+1D400…`), with the reserved-codepoint holes table (`ℎ ℬ ℰ ℱ ℋ ℐ
      ℒ ℳ ℛ ℂ ℍ ℕ ℙ ℚ ℝ ℤ ℭ ℌ ℑ ℜ ℨ`, plus the `Roman` variant which is
      `mathvariant="normal"` on a single-letter `mi`). Unit test the table
      against the Unicode chart for one letter per variant and every hole.
- [x] (2026-09-21) **Escaping**: `<`, `&`, `>` in `mo`/`mi`/`mtext`/`annotation`.
- [x] (2026-09-21; `Normalized` gained a `text` field so the writer can emit the `<annotation>`; `-` in runs is emitted as U+2212) Implement `crates/quarto-math/src/mathml.rs`, `Target::MathMl` in
      `convert.rs`, `render()` arm, crate docs. Runs `split_run` on every
      `Run` (the pass the parent plan reserved for this).
- [x] (2026-09-21: `cargo xtask verify --skip-hub-build` green; `cargo check --target wasm32-unknown-unknown -p quarto-math` clean) `cargo xtask verify --skip-hub-build` (quarto-math only), then the
      full verify once Phase 3 touches `quarto-core`.

**Found while snapshotting (2026-09-21):** the normalizer dropped primes
(`f'` → `f`; the committed Typst snapshot for `basic/prime` read
`f ( x ) = f ( x )`). mitex parses `f'` as an attachment with no `^`/`_`
operator and `attach` returned only the base. Fixed under TDD in this
phase (5 tests in `normalize.rs`): a prime is a superscript `Sym`, repeated
primes merge into `″`/`‴`/`⁗`, and `x'^2` shares the superscript slot
instead of raising a double-superscript error. Six snapshots changed
(`basic/prime`, `scripts/prime-with-sup` × normalize/OMML/Typst). The
same fixture exposes a separate mitex quirk, `f''` after `=` taking `= f`
as its base, filed as bd-0mzhnxft.

### Phase 3 — `MathMlStage` for `format: html` (new strand, stacked on #706 + Phase 1)

- [x] (2026-09-21; 5 tests appended to `math_mode_pipeline.rs`) **End-to-end tests in `math_mode_pipeline.rs`**: `html-math-method:
      mathml` with inline + display + numbered math → `<math` present, the
      `\(`/`\[` delimiters absent, no MathJax/KaTeX loader, the numbered
      equation carries its sibling label; an expression with an unknown
      command → verbatim TeX span retained, a `Q-22-1` warning in the
      render diagnostics, and (hybrid, see decisions) the MathJax loader
      present; a math-free document → no `<math`, no loader; a website with
      one mathml page and one math-free page.
- [x] (2026-09-21; 5 tests in `math_ml.rs`; the walker is now the shared `crate::ast_walk::for_each_inline_mut`, which `EquationNumberStage` uses too) **Stage unit tests**: converts `Inline::Math` inside paragraphs,
      headers, list items, table cells and `CustomNode` slots (reuse the
      walker shape of `doc_has_math`); passes `Math.text_source` (falling
      back to `source_info`) so diagnostics point into the `.qmd`; leaves
      failed expressions as `Inline::Math`.
- [x] (2026-09-21) **`MathJsStage` hybrid test**: for method `mathml`, injection happens
      iff an `Inline::Math` survives the MathML stage.
- [x] (2026-09-21; conversion errors are downgraded to warnings since the page still renders via MathJax; quarto-core now depends on quarto-math) Implement `crates/quarto-core/src/stage/stages/math_ml.rs`, registered
      right after `EquationNumberStage`, gated on `MathMethod::MathMl`, using
      `quarto_math::convert(text, mode, Target::MathMl, &text_source,
      Spec::builtin())`, `ctx.add_diagnostics` for every conversion.
      Add `"math-ml"` to `Q2_PREVIEW_STAGE_EXCLUDED` (the preview renders
      `Inline::Math` with KaTeX client-side).
- [x] (2026-09-21) `MathEngine::from_meta` (via `MathMethod`): `MathMl` maps to the
      MathJax default engine when leftovers exist, `None` otherwise.
- [x] (2026-09-21; linked from `Q-22-1`'s page; the HTML format guides are not in the docs sidebar today, same as `themes.qmd`) Docs: new `docs/guides/formats/html/math.qmd` documenting
      `html-math-method` (`mathjax` default, `katex`, `mathml` and its
      browser/font caveats, the hybrid fallback and the `Q-22-*` warnings);
      link `Q-22-1`'s page to it.
- [x] (2026-09-21, see below) **End-to-end browser verification** (required by CLAUDE.md): render
      the probe document with `cargo run --bin q2 -- render`, open it in a
      real browser (Chrome MCP, or the headless Playwright fallback), confirm
      `document.querySelector('math')` has a non-zero box and the label sits
      on the right of the numbered equation; screenshot. Record invocation
      and output snippet here.
- [x] (2026-09-21: full `cargo xtask verify` green — 14248 Rust tests, ts-packages, hub-client build incl. the WASM leg with quarto-math linked in, hub-client tests) Full `cargo xtask verify` (quarto-core changed; hub-client WASM leg
      picks up the new quarto-math code path).

**End-to-end verification (2026-09-21, real binary + headless Chromium,
output inspected).** Probe document with `html-math-method: mathml`:
inline `$x^2 + \frac{a}{b}$` and `$\alpha \leq \beta$`, a numbered
display equation (`\sum … \int …` with `{#eq-one}`), a display block with
`pmatrix` + `cases`, an `@eq-one` reference, and one deliberately
unconvertible `$x + \bogus y$`. `cargo run --bin q2 -- render doc.qmd`
prints exactly one diagnostic:

```
Warning: [Q-22-1] Unknown Math Command
   ╭─[ doc.qmd:20:40 ]
20 │ See @eq-one. This one falls back: $x + \bogus y$.
   │                                        ───┬──
   │                                           ╰──── unknown command `\bogus`
```

The HTML has four `<math xmlns=…>` elements (two `display="block"`), the
numbered equation as
`…</math></span><span class="quarto-eq-number">(1)</span></span>`, the
leftover as `<span class="math inline">\(x + \bogus y\)</span>`, and one
MathJax config whose `skipHtmlTags` now lists `annotation`. Headless
Chromium (Playwright, MathJax allowed to load from the CDN) reports: the
four native `<math>` boxes have heights 22/16/27/39 px; exactly one
MathJax-typeset element exists (`x+\bogus y`); the `(1)` label sits to
the right of the equation, flush with the row's right edge, vertically
centred on it. Screenshot reviewed: fractions, the sum with under/over
limits, the integral with side limits, the fenced matrix and the cases
brace all render natively.

The first browser pass caught a real bug: q2's MathJax config overrode
MathJax's default `skipHtmlTags`, dropping `annotation`, so the fallback
loader re-typeset the `\begin{pmatrix}`/`\begin{cases}` text inside the
converted math's annotations (zero-size assistive MathML in the DOM).
Fixed with a unit test on the config and an end-to-end assertion.

## Decisions (all settled 2026-09-21, each as recommended)

1. **Attribute key.** `quarto-eq-number` as proposed, or a shorter
   `eq-number`. Recommendation: `quarto-eq-number`, consistent with the
   other reserved names.
2. **Hybrid fallback for `mathml`.** When an expression fails conversion:
   (a) leave verbatim TeX and load MathJax for the page so it still renders
   (recommended: the reader sees math; the author sees the `Q-22` warning
   and can fix the source to drop the dependency), or (b) leave verbatim
   TeX unrendered, JS-free at any cost. Both keep the warning.
3. **`\cancel`.** `menclose notation="updiagonalstrike"` is not MathML Core
   (Firefox renders it, Chrome ignores the strike but shows the content).
   Recommendation: emit it anyway, no warning, document the limitation;
   two corpus uses.
4. **Math fonts.** Native MathML quality depends on an OpenType MATH font
   being available to the browser. Recommendation for v1: document it, do
   not ship a font. Revisit if users report rendering gaps.
5. **`plain` semantics.** Quarto 1 delegates `plain` to Pandoc's
   text-approximation writer; q2 today emits raw TeX with no loader.
   Recommendation: keep q2's behaviour (raw TeX, `Qquad` numbering) and
   leave a text-approximation writer out of scope.

## Out of scope

- Section-prefixed equation numbers (`2.3`); the attribute carries whatever
  the crossref index produces, and the index produces a flat number today.
- Numbering for the native docx/Typst/LaTeX writers (`Writer` strategy is
  a placeholder until those writers exist; #704's Pandoc route uses Quarto
  1's Lua numbering).
- A Pandoc-style `plain` text-approximation writer.
- MathML in the q2-preview iframe (KaTeX stays).

## References

- Quarto 1 numbering: `external-sources/quarto-cli/src/resources/filters/crossref/equations.lua` (`renderEquation`, `eqTag`, `eqQquad`).
- Pandoc HTML writer math dispatch: `external-sources/pandoc/src/Text/Pandoc/Writers/HTML.hs` (`MathML` arm; `annotateMML`). texmath itself stays off limits (GPL, parent plan).
- Existing math-mode stage and tests: `crates/quarto-core/src/stage/stages/math_js.rs`, `crates/quarto-core/tests/integration/math_mode_pipeline.rs`.
- Post-filter stage precedent: `CodeHighlightStage`.
- Phase contract: `claude-notes/designs/transform-pipeline-phases.md`.
- MathML Core: https://www.w3.org/TR/mathml-core/
