/**
 * Helpers for extracting content from the hub-client preview iframe.
 *
 * These run against the live browser page after a project has been loaded
 * and rendered through the full Automerge → VFS → WASM → Preview pipeline.
 */

import type { Page } from '@playwright/test';
import type {} from './testHooks';

/**
 * Wait for the preview iframe to render content.
 *
 * For html-style previews (the default) the DoubleBufferedIframe component
 * mounts an `iframe.preview-active` whose body is populated when render
 * completes. For `format: q2-debug`, the renderer mounts an AstIframe
 * whose `src` ends in `ast-renderer.html`; we wait for that iframe and
 * for its body to receive content from the postMessage flow.
 *
 * If `consoleErrors` is provided, the wait will abort early when a fatal
 * browser error is detected (e.g. WebSocket failure, WASM crash), avoiding
 * a long timeout with no diagnostic info.
 */
export type PreviewIframeKind = 'html' | 'q2-debug';

export function previewIframeSelector(kind: PreviewIframeKind): string {
  return kind === 'q2-debug'
    ? 'iframe[src*="ast-renderer.html"]'
    : 'iframe.preview-active';
}

export async function waitForPreviewRender(
  page: Page,
  opts: {
    timeout?: number;
    consoleErrors?: string[];
    kind?: PreviewIframeKind;
  } = {},
): Promise<void> {
  const timeout = opts.timeout ?? 30000;
  const consoleErrors = opts.consoleErrors;
  const iframeSelector = previewIframeSelector(opts.kind ?? 'html');

  // Poll for render completion, but also check for fatal console errors
  // so we can fail fast with a useful message instead of timing out.
  const pollInterval = 250;
  const deadline = Date.now() + timeout;

  while (Date.now() < deadline) {
    // Check if any fatal console errors have been collected
    if (consoleErrors && consoleErrors.length > 0) {
      // Only treat unrecoverable WASM traps as immediately fatal.
      // Lua panics are expected control flow on wasm32 (LUAI_THROW is
      // rewired to a Rust panic caught by rust_lua_protected_call) and
      // are suppressed by a custom panic hook in wasm-quarto-hub-client
      // (see claude-notes/plans/2026-04-16-suppress-lua-panic-noise.md),
      // so they should not reach consoleErrors at all. Network errors
      // (500s) and out-of-date builds may still produce transient noise.
      const fatal = consoleErrors.find(
        (e) =>
          e.includes('unreachable') ||
          e.includes('RuntimeError'),
      );
      if (fatal) {
        throw new Error(
          `Fatal browser error during render wait: ${fatal}\nAll console errors:\n${consoleErrors.join('\n')}`,
        );
      }
    }

    // Check if the preview iframe has rendered content
    const rendered = await page.evaluate((selector) => {
      const iframe = document.querySelector(selector) as HTMLIFrameElement | null;
      if (!iframe?.contentDocument?.body) return false;
      return iframe.contentDocument.body.innerHTML.length > 0;
    }, iframeSelector);

    if (rendered) return;

    await page.waitForTimeout(pollInterval);
  }

  // Timed out — build a useful error message
  const errorContext = consoleErrors?.length
    ? `\nConsole errors:\n${consoleErrors.join('\n')}`
    : '\nNo console errors captured.';
  throw new Error(
    `Timed out after ${timeout}ms waiting for preview iframe to render.${errorContext}`,
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
    await window.__quartoTestReady;
    const hooks = window.__quartoTest;
    if (!hooks) throw new Error('__quartoTest missing — rebuild with VITE_E2E=1');
    const renderer = hooks.wasmRenderer;

    // Discover user grammars from the VFS so the re-render matches the
    // in-Preview render (Preview.tsx forwards a project-file list +
    // resolvers from automergeSync; bd-izfv made the project-render
    // path actually honor that handle). Without this, fixtures that
    // depend on `_quarto/grammars/<lang>/` render unhighlighted here
    // even though the live iframe renders them correctly.
    //
    // `vfs_list_files` returns the VFS-absolute form (`/project/...`);
    // `discoverUserGrammars` expects project-relative paths with no
    // leading slash, and the resolver callbacks below mirror that
    // shape (matches `Preview.tsx` → `automergeSync` wiring).
    const VFS_PROJECT_PREFIX = '/project/';
    const stripPrefix = (vfsPath: string): string | null =>
      vfsPath.startsWith(VFS_PROJECT_PREFIX)
        ? vfsPath.slice(VFS_PROJECT_PREFIX.length)
        : null;
    const toVfsPath = (relPath: string): string =>
      relPath.startsWith('/') ? relPath : `${VFS_PROJECT_PREFIX}${relPath}`;

    const listing = renderer.vfsListFiles();
    const projectFilePaths: string[] = listing.success
      ? (listing.files ?? [])
          .map(stripPrefix)
          .filter((p): p is string => p !== null)
      : [];

    const result = await renderer.renderToHtml({
      documentPath: docPath,
      userGrammars: {
        files: projectFilePaths,
        getBinaryContent: async (path: string) => {
          const r = renderer.vfsReadBinaryFile(toVfsPath(path));
          if (!r.success || typeof r.content !== 'string') return null;
          // Decode base64 → Uint8Array on the page (Buffer isn't available).
          const binary = atob(r.content);
          const bytes = new Uint8Array(binary.length);
          for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
          return bytes;
        },
        getTextContent: async (path: string) => {
          const r = renderer.vfsReadFile(toVfsPath(path));
          return r.success && typeof r.content === 'string' ? r.content : null;
        },
      },
    });
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

    await window.__quartoTestReady;
    const hooks = window.__quartoTest;
    if (!hooks) throw new Error('__quartoTest missing — rebuild with VITE_E2E=1');
    const renderer = hooks.wasmRenderer;
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
    await window.__quartoTestReady;
    const hooks = window.__quartoTest;
    if (!hooks) throw new Error('__quartoTest missing — rebuild with VITE_E2E=1');
    const result = await hooks.wasmRenderer.renderToHtml({ documentPath: docPath });
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
