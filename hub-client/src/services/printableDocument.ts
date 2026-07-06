/**
 * "Open printable version" (issue #315, bd-vhdknrvl).
 *
 * The live preview renders in-scope documents (`format: q2-preview`,
 * `format: revealjs`) through the React/AST pipeline inside a sandboxed
 * iframe — there is no standalone document to print, and the browser's
 * "Print Frame" affordance produces a clipped, single-page result (or
 * is absent entirely in Chrome). See the plan at
 * `claude-notes/plans/2026-07-06-issue-315-preview-printing.md`.
 *
 * Instead we render a **standalone printable document** on demand
 * (`render_printable`, which coerces the preview format to its
 * HTML-output equivalent and renders through the HTML pipeline),
 * inline every VFS-backed asset so it is self-contained
 * (`makeSelfContainedHtml`), force reveal decks into their paginated
 * print layout (`forceRevealPrintMode`), and open it in a **new
 * top-level tab**. As a real top-level document the browser paginates
 * it natively and its `@media print` rules apply, so the user gets a
 * correct multi-page print/PDF via ⌘P.
 */

import {
  renderPrintable,
  vfsReadFile,
  vfsReadBinaryFile,
} from '@quarto/preview-runtime';
import {
  makeSelfContainedHtml,
  type SelfContainedReaders,
} from '@quarto/preview-renderer/utils/makeSelfContainedHtml';
import { forceRevealPrintMode } from '@quarto/preview-renderer/utils/revealPrintMode';

/** Bind the self-contained inliner's readers to the live WASM VFS. */
function vfsReaders(): SelfContainedReaders {
  return {
    readText: (p) => {
      const r = vfsReadFile(p);
      return r.success && r.content != null ? r.content : null;
    },
    readBinaryBase64: (p) => {
      const r = vfsReadBinaryFile(p);
      return r.success && r.content != null ? r.content : null;
    },
  };
}

/**
 * Preview formats whose printable document is a reveal deck (and so
 * needs `view:"print"` forced). Both the explicit `q2-slides` preview
 * pseudo-format and `revealjs` land here.
 */
export function isPrintableSlidesFormat(format: string | null): boolean {
  return format === 'revealjs' || format === 'q2-slides';
}

/**
 * Format the printable-render `RenderResponse` into a final,
 * self-contained HTML string. Pure (given the injected readers) and
 * throws on a failed render, so it is unit-testable in isolation from
 * the WASM render and `window.open`.
 */
export function buildPrintableHtml(
  html: string | undefined,
  currentFilePath: string,
  format: string | null,
  readers: SelfContainedReaders,
): string {
  if (!html) {
    throw new Error('Printable render produced no HTML');
  }
  let out = makeSelfContainedHtml(html, currentFilePath, readers);
  if (isPrintableSlidesFormat(format)) {
    out = forceRevealPrintMode(out);
  }
  return out;
}

/**
 * Render, inline, and open a printable version of the current document
 * in a new top-level tab. Rejects if the render fails or the browser
 * blocks the pop-up (so the caller can surface a message).
 */
export async function openPrintableDocument(
  currentFilePath: string,
  format: string | null,
): Promise<void> {
  const resp = await renderPrintable(currentFilePath);
  if (!resp.success) {
    throw new Error(
      resp.error ? String(resp.error) : 'Failed to render printable document',
    );
  }

  const html = buildPrintableHtml(
    resp.html,
    currentFilePath,
    format,
    vfsReaders(),
  );

  const blob = new Blob([html], { type: 'text/html' });
  const url = URL.createObjectURL(blob);
  const win = window.open(url, '_blank');
  if (!win) {
    URL.revokeObjectURL(url);
    throw new Error(
      'Could not open a new tab — please allow pop-ups for this site and try again.',
    );
  }
  // Release the blob once the new tab has had time to load off it. The
  // browser retains the bytes while the load is in flight; we keep a
  // generous window for slow reveal decks.
  window.setTimeout(() => URL.revokeObjectURL(url), 60_000);
}
