import { extractMetaString } from '@quarto/preview-renderer/framework';

/**
 * Decide which preview branch `PreviewRouter` mounts for a document, from
 * the format string `MetadataMergeStage` wrote into the parsed AST's
 * `meta.format` (the normalised `Format::target_format`).
 *
 * Returns the format `ReactPreview` should render with, or `null` to mount
 * the full-DOM `Preview` (MorphIframe) instead.
 *
 * The rule (bd-kltzdhle, D2 in
 * `claude-notes/plans/2026-09-09-hub-client-default-q2-preview.md`):
 *
 * - `html` — which is also what the WASM reports for a document with no
 *   `format:` key, or a `format: html: {…}` map — routes to `q2-preview`.
 *   This is the JS half of the `q2 preview` default-format substitution
 *   (`map_format_for_preview` in `crates/wasm-quarto-hub-client/src/lib.rs`);
 *   the render half is `ReactPreview` passing `preferPreviewFormat` to
 *   the WASM so the pipeline applies the same mapping. Keep the two in
 *   step.
 * - `q2-html-render` is the explicit opt-out into the full-DOM renderer.
 * - Every other `q2-*` pseudo-format and `revealjs` pass through to the
 *   React side unchanged.
 * - Anything else (`pdf`, `docx`, extension formats such as `acm-html`, …)
 *   falls back to the full-DOM renderer, which renders it through the
 *   HTML pipeline as before. Whether html-based extension formats should
 *   also preview as q2-preview is an open question (bd-vhd3lugq).
 */
export function getQ2Format(astJson: string): string | null {
  try {
    const ast = JSON.parse(astJson);
    const formatStr = extractMetaString(ast?.meta?.format);
    if (!formatStr) return null;
    if (formatStr === 'q2-html-render') return null;
    if (formatStr === 'html') return 'q2-preview';
    if (formatStr.startsWith('q2-') || formatStr === 'revealjs') return formatStr;
    return null;
  } catch (err) {
    console.error('[PreviewRouter] Failed to parse AST:', err);
    return null;
  }
}
