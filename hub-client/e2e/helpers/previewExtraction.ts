/**
 * Helpers for extracting content from the hub-client preview iframe.
 *
 * These run against the live browser page after a project has been loaded
 * and rendered through the full Automerge → VFS → WASM → Preview pipeline.
 */

import type { Page } from '@playwright/test';

/**
 * Wait for the preview iframe to render content.
 *
 * The DoubleBufferedIframe component injects a `<!-- render-<timestamp> -->`
 * comment on each render. We wait for:
 * 1. An iframe with class `preview-active` to exist
 * 2. Its body to have non-empty innerHTML (content rendered)
 */
export async function waitForPreviewRender(
  page: Page,
  opts: { timeout?: number } = {},
): Promise<void> {
  const timeout = opts.timeout ?? 30000;

  // Wait for the active preview iframe to have rendered content
  // The render marker comment (<!-- render-XXX -->) indicates completion
  await page.waitForFunction(
    () => {
      const iframe = document.querySelector('iframe.preview-active') as HTMLIFrameElement | null;
      if (!iframe?.contentDocument?.body) return false;
      const html = iframe.contentDocument.body.innerHTML;
      return html.length > 0;
    },
    { timeout },
  );
}

/**
 * Get the raw rendered HTML by re-rendering the document via WASM.
 *
 * The browser's DOM serialization loses DOCTYPE and wraps inline text
 * in data-sid spans, so we can't reliably match raw HTML patterns from
 * the iframe content. Instead we do a fresh WASM render (VFS is already
 * populated) and return the raw HTML string.
 */
export async function getPreviewHtml(
  page: Page,
  documentPath: string,
): Promise<string> {
  return page.evaluate(async (docPath) => {
    const renderer = await import('/src/services/wasmRenderer.ts');
    const result = await renderer.renderToHtml({ documentPath: docPath });
    return result.html ?? '';
  }, documentPath);
}

/**
 * Get combined CSS from all local stylesheets referenced in the preview.
 *
 * Parses <link rel="stylesheet"> tags from the preview HTML, reads each
 * local stylesheet from VFS via the wasmRenderer module, and returns
 * the concatenated CSS.
 */
export async function getPreviewCss(page: Page): Promise<string> {
  return page.evaluate(async () => {
    const iframe = document.querySelector('iframe.preview-active') as HTMLIFrameElement | null;
    if (!iframe?.contentDocument) {
      throw new Error('No active preview iframe found');
    }

    const renderer = await import('/src/services/wasmRenderer.ts');
    const links = iframe.contentDocument.querySelectorAll('link[rel="stylesheet"]');
    let combinedCss = '';

    for (const link of links) {
      const href = link.getAttribute('href');
      if (!href || href.startsWith('http://') || href.startsWith('https://') || href.startsWith('//')) {
        continue;
      }

      // Handle data: URIs (CSS is inlined by iframePostProcessor)
      if (href.startsWith('data:')) {
        // Extract CSS from data URI: data:text/css;base64,... or data:text/css,...
        const commaIdx = href.indexOf(',');
        if (commaIdx === -1) continue;
        const meta = href.slice(0, commaIdx);
        const data = href.slice(commaIdx + 1);
        if (meta.includes('base64')) {
          combinedCss += atob(data) + '\n';
        } else {
          combinedCss += decodeURIComponent(data) + '\n';
        }
        continue;
      }

      // Try reading from VFS
      const vfsPath = href.startsWith('/') ? href : `/project/${href}`;
      try {
        const result = renderer.vfsReadFile(vfsPath);
        if (result.success && result.content) {
          combinedCss += result.content + '\n';
        }
      } catch {
        // CSS file not readable from VFS — may be post-processed
      }
    }

    return combinedCss;
  });
}

/**
 * Diagnostic info from a render result.
 */
export interface RenderDiagnostic {
  kind: string;
  title: string;
}

/**
 * Get render diagnostics by re-rendering the document via page.evaluate.
 *
 * Since the Preview component doesn't expose its last render result to
 * the global scope, we perform a fresh render to capture diagnostics.
 * The VFS is already populated from the Automerge sync, so this is fast.
 */
export async function getRenderDiagnostics(
  page: Page,
  documentPath: string,
): Promise<{
  success: boolean;
  error?: string;
  diagnostics: RenderDiagnostic[];
  warnings: RenderDiagnostic[];
}> {
  return page.evaluate(async (docPath) => {
    const renderer = await import('/src/services/wasmRenderer.ts');
    const result = await renderer.renderToHtml({ documentPath: docPath });
    return {
      success: result.success,
      error: result.error,
      diagnostics: (result.diagnostics ?? []).map((d: { kind: string; title: string }) => ({
        kind: d.kind,
        title: d.title,
      })),
      warnings: (result.warnings ?? []).map((d: { kind: string; title: string }) => ({
        kind: d.kind,
        title: d.title,
      })),
    };
  }, documentPath);
}
