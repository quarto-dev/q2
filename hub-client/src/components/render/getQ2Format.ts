import { extractMetaString } from '@quarto/preview-renderer/framework';
import type { ResolvePandocFormatsResponse } from '@quarto/preview-runtime';

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

/**
 * What `PreviewRouter` mounts for a document (pandoc-host H5, design D8.4/D8.7). An enum so a
 * later "pdf-preview" mode fits.
 *
 * - `react`: `ReactPreview` renders `format` (the `q2-*` pseudo-formats, `revealjs`, `html`).
 * - `dom`: the full-DOM `Preview` (MorphIframe) renders the HTML pipeline's output.
 * - `download`: the document's own format cannot be previewed but pandoc.wasm (or the native
 *   render in the embed) can produce it: no preview, a "Download <type>" button.
 * - `pdf`: the document's own format is `pdf` and this build compiles it in the browser: the pdf.js
 *   viewer (the third kind of preview iframe) shows the compiled PDF.
 * - `neither`: nothing can show or produce it: no preview, the control is disabled and
 *   the pane says why.
 */
export type PreviewMode =
  | { mode: 'react'; format: string }
  | { mode: 'dom' }
  | { mode: 'download'; formatKey: string }
  | { mode: 'pdf'; formatKey: 'pdf' }
  | { mode: 'neither'; formatKey: string };

export type FormatClassResolver = (path: string) => ResolvePandocFormatsResponse | null;

export interface ClassifyDeps {
  /** The Rust-owned project-aware resolver (`resolvePandocFormats`). */
  resolve: FormatClassResolver;
  /** True when "Download as" can produce this format key in this build. */
  canDownload: (formatKey: string) => boolean;
  /** True when this build compiles PDF in the browser, so a `pdf` document gets the viewer. */
  canPreviewPdf?: () => boolean;
}

/**
 * The three-class classifier. `getQ2Format` keeps deciding the React side; for everything it
 * returns `null` for, the class of the document's own (first) format decides: `preview` and
 * unknown keep the full-DOM renderer, `download` becomes the click-only download mode (when this
 * build can produce it), `neither` mounts neither preview.
 */
export function classifyPreviewMode(astJson: string, path: string | undefined, deps: ClassifyDeps): PreviewMode {
  const react = getQ2Format(astJson);
  if (react) return { mode: 'react', format: react };
  try {
    const ast = JSON.parse(astJson);
    if (extractMetaString(ast?.meta?.format) === 'q2-html-render') return { mode: 'dom' };
  } catch {
    return { mode: 'dom' };
  }
  if (!path) return { mode: 'dom' };
  const resolved = deps.resolve(path);
  if (!resolved || !resolved.success || resolved.formats.length === 0) return { mode: 'dom' };
  const first = resolved.formats[0];
  switch (first.class) {
    case 'preview':
      return { mode: 'dom' };
    case 'download':
      if (first.key === 'pdf' && deps.canDownload('pdf') && deps.canPreviewPdf?.()) return { mode: 'pdf', formatKey: 'pdf' };
      return deps.canDownload(first.key) ? { mode: 'download', formatKey: first.key } : { mode: 'dom' };
    case 'neither':
      return { mode: 'neither', formatKey: first.key };
  }
}
