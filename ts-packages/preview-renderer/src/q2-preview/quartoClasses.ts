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

// Callout — TS Quarto / Bootstrap-aligned class vocabulary (matches
// `CalloutResolveTransform` post-2026-05-22 rewrite). CalloutResolveTransform
// itself is still excluded from the q2-preview pipeline, so the
// Callout React component (`./custom/Callout.tsx`) emits the same
// classes the resolver would have produced for the HTML pipeline.
//
// Source: crates/quarto-core/src/transforms/callout_resolve.rs
//   :220-241 (outer), :345-380 (titled-path header), :404-426 (untitled-path body)
// Mirror in TS Quarto: src/resources/filters/modules/callouts.lua
//   :247-260 (outer), :286-289 (header), :336-337 (untitled body).
export const CALLOUT = 'callout';
export const CALLOUT_TYPE_PREFIX = 'callout-'; // callout-note, callout-warning, callout-tip, callout-important, callout-caution
export const CALLOUT_STYLE_PREFIX = 'callout-style-'; // callout-style-default | callout-style-simple — ALWAYS emitted
export const CALLOUT_TITLED = 'callout-titled'; // outer; present when callout has a title
export const NO_ICON = 'no-icon'; // outer; present when icon=false OR type unknown
export const CALLOUT_EMPTY_CONTENT = 'callout-empty-content'; // outer; present when body has no content
export const CALLOUT_COLLAPSE = 'callout-collapse'; // collapse wrapper; co-class with COLLAPSE_BS and SHOW_BS
export const CALLOUT_HEADER = 'callout-header'; // titled path only
export const CALLOUT_TITLE_CONTAINER = 'callout-title-container'; // titled path; inside .callout-header
export const CALLOUT_FLEX_FILL = 'flex-fill'; // co-class on .callout-title-container
export const CALLOUT_ICON_CONTAINER = 'callout-icon-container';
export const CALLOUT_ICON = 'callout-icon';
export const CALLOUT_BODY_CONTAINER = 'callout-body-container';
export const CALLOUT_BODY = 'callout-body'; // titled: co-class on .callout-body-container; untitled: standalone outer wrap

// Bootstrap utility classes used by the canonical callout markup.
export const BS_D_FLEX = 'd-flex';
export const BS_ALIGN_CONTENT_CENTER = 'align-content-center';
export const BS_COLLAPSE = 'collapse';
export const BS_SHOW = 'show';

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

// Accessibility — TS Quarto prepends a `<span class="screen-reader-only">`
// to a titled callout's title inlines so screen readers announce the
// callout type ("Note", "Warning", …) even though the visible title is
// user-supplied. callouts.lua:271-275 / callout_resolve.rs (mirror).
export const SCREEN_READER_ONLY = 'screen-reader-only';
