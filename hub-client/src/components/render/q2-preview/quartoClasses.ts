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
 * 2B stub scope: section / footnotes / appendix only. Plan 2C extends
 * with the Quarto-feature taxonomy (callout, theorem, proof, quarto-xref,
 * etc.) when the CustomNode components ship.
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
