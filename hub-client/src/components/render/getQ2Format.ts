import type { ResolvePandocFormatsResponse } from '@quarto/preview-runtime';
import { TYPST_PDF_KEY } from '../../pandoc/formatKeys';

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
  | { mode: 'pdf'; formatKey: typeof TYPST_PDF_KEY }
  | { mode: 'neither'; formatKey: string };

export type FormatClassResolver = (path: string) => ResolvePandocFormatsResponse | null;

export interface ClassifyDeps {
  /** The Rust-owned project-aware resolver (`resolvePandocFormats`). */
  resolve: FormatClassResolver;
  /** True when "Download as" can produce this format key in this build. */
  canDownload: (formatKey: string) => boolean;
  /** True when this build can preview PDF (browser chain, or the embed's native route), so a `pdf` document gets the viewer. */
  canPreviewPdf?: () => boolean;
}

/**
 * Decide which preview `PreviewRouter` mounts for the document at `path`. TypeScript decides
 * nothing about the format: the Rust resolver (project-aware, chapter scope, reading the editor
 * buffer through `deps.resolve`) returns the document's own `key`, its `class` and the
 * `pseudoFormat` the live preview renders it with, and this maps them to a mode.
 *
 * - `pseudoFormat` set (`html` → `q2-preview`, `revealjs`, the `q2-*` formats): `react`.
 *   This is the JS half of the `q2 preview` default-format substitution
 *   (`preview_pseudo_format` in `quarto-core`); the render half is `ReactPreview` passing
 *   `preferPreviewFormat` to the WASM so the pipeline applies the same mapping.
 * - `q2-html-render` is the explicit opt-out into the full-DOM renderer (`dom`).
 * - Otherwise the class of the resolved `key` decides: `preview` (extension formats such as
 *   `acm-html`, unknown keys) keeps the full-DOM renderer, `download` becomes the click-only
 *   download mode (or the `pdf` viewer for `typst-pdf`) when this build can produce it,
 *   `neither` mounts neither preview.
 *
 * Never reads `formats[0]`: with a requested format that is not declared it would be wrong.
 */
export function classifyPreviewMode(path: string | undefined, deps: ClassifyDeps): PreviewMode {
  if (!path) return { mode: 'dom' };
  const resolved = deps.resolve(path);
  if (!resolved || !resolved.success) return { mode: 'dom' };
  if (resolved.key === 'q2-html-render') return { mode: 'dom' };
  if (resolved.pseudoFormat) return { mode: 'react', format: resolved.pseudoFormat };
  switch (resolved.class) {
    case 'preview':
      return { mode: 'dom' };
    case 'download':
      if (resolved.key === TYPST_PDF_KEY && deps.canDownload(TYPST_PDF_KEY) && deps.canPreviewPdf?.()) return { mode: 'pdf', formatKey: TYPST_PDF_KEY };
      return deps.canDownload(resolved.key) ? { mode: 'download', formatKey: resolved.key } : { mode: 'dom' };
    case 'neither':
      return { mode: 'neither', formatKey: resolved.key };
  }
}
