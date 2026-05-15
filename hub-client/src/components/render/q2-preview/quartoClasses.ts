/**
 * Class-name constants q2-preview emits to match Quarto's HTML pipeline.
 *
 * These mirror the strings the Rust transforms / writers emit; theme
 * CSS targets them, so any drift breaks visual parity. Re-verify on any
 * major Rust transform refactor — the §"Class-compatibility test" in
 * `q2-preview.integration.test.tsx` catches drift between this file and
 * what the components emit, and the smoke-all fixtures catch drift
 * between Rust output and these constants at integration time.
 *
 * 2B stub scope: section / footnotes / appendix.
 * 2C extends with the Quarto-feature taxonomy (callout, theorem, proof,
 * quarto-xref) consumed by the CustomNode components.
 */

// Section / level — crates/pampa/src/transforms/sectionize.rs:114
// (Rendered by q2-preview's Div.tsx; not a CustomNode, but worth pinning
// for class-compatibility tests.)
export const SECTION = 'section';
export const SECTION_LEVEL_PREFIX = 'level'; // level1, level2, ..., level6

// Footnotes — emitted by FootnotesTransform (now included in q2-preview's
// pipeline; see plan §"Pipeline change: include FootnotesTransform").
// Source: crates/quarto-core/src/transforms/footnotes.rs:26-35,440-460
export const FOOTNOTES = 'footnotes'; // outer Div(class="footnotes")
export const FOOTNOTE_REF = 'footnote-ref'; // <a> inside the inline <sup>
export const FOOTNOTE_BACK = 'footnote-back'; // backlink <a> inside each <li>

// Appendix container — emitted by AppendixStructureTransform (now included
// in q2-preview's pipeline; see plan §"Pipeline change: include AppendixStructureTransform").
// Source: crates/quarto-core/src/transforms/appendix.rs:244-365
export const QUARTO_APPENDIX = 'quarto-appendix'; // outer container Div
export const QUARTO_BIBLIOGRAPHY = 'quarto-bibliography'; // bib section (inert until Citeproc)
export const QUARTO_REUSE = 'quarto-reuse'; // license section
export const QUARTO_COPYRIGHT = 'quarto-copyright'; // copyright section
export const QUARTO_CITATION = 'quarto-citation'; // how-to-cite section

// Callout — emitted by CalloutResolveTransform (excluded from q2-preview;
// q2-preview keeps the Callout CustomNode wrapper, but the class names
// must match for theme CSS compatibility).
// Source: crates/quarto-core/src/transforms/callout_resolve.rs
//   :170,172,175,199,215,226,234
export const CALLOUT = 'callout';
export const CALLOUT_TYPE_PREFIX = 'callout-'; // callout-note, callout-warning, callout-tip, callout-important, callout-caution
export const CALLOUT_APPEARANCE_PREFIX = 'callout-appearance-'; // callout-appearance-{simple,minimal} — `default` is omitted
export const CALLOUT_COLLAPSE = 'callout-collapse';
export const CALLOUT_HEADER = 'callout-header';
export const CALLOUT_TITLE_CONTAINER = 'callout-title-container';
export const CALLOUT_FLEX_FILL = 'flex-fill'; // co-class on .callout-title-container — callout_resolve.rs:215
export const CALLOUT_ICON_CONTAINER = 'callout-icon-container';
export const CALLOUT_ICON = 'callout-icon';
export const CALLOUT_BODY_CONTAINER = 'callout-body-container';
export const CALLOUT_BODY = 'callout-body';

// Theorem / Proof — crates/quarto-core/src/transforms/crossref_render.rs:346,482,537
//   theorem env class is computed via theoremEnvFor() in ./theoremEnvs.ts
//   (8-entry mapping — port of theorem_env_for at crossref_render.rs:388-400).
//   No `proof-title` class: the proof label is an inline `<em>Proof.</em>`
//   (italic), not a wrapped Span — see render_proof at crossref_render.rs:534-585.
export const THEOREM = 'theorem';
export const THEOREM_TITLE = 'theorem-title';
export const PROOF = 'proof';

// Equation — crates/quarto-core/src/transforms/crossref_render.rs:601-650
//   No specific class; preserves user attr (typically just `id="eq-..."`).
//   q2-preview's Equation.tsx wraps the Math in `<span id={id}>` with no
//   added classes, matching the Rust output.

// FloatRefTarget — crates/quarto-core/src/transforms/float_ref_target.rs:240,315
//   No classes added; preserves user attr verbatim. In Rust HTML output the
//   figure subtype maps to a native `<figure>` (no class), other subtypes
//   to `<div>` (no class). Identifier carries on the `id` attribute.

// CrossrefResolvedRef — crates/quarto-core/src/transforms/crossref_render.rs:707
export const QUARTO_XREF = 'quarto-xref';
