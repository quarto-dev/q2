import { useEffect, useMemo, useRef, useState } from 'react';
import { vfsReadFile } from '@quarto/preview-runtime';
import { DEFAULT_CSS_ARTIFACT_PATH } from '@quarto/preview-renderer/types/artifactPaths';
import { isBinaryPath } from '../../../../quarto-hub-sandboxed-preview/src/assetPolicy';
import { buildProxyAssetManifest } from './proxyAssetManifest';

interface Q2SandboxedPreviewIframeProps {
  astJson: string;
  currentFilePath: string;
  /**
   * Three-way theme fingerprint, same semantics as `Q2PreviewIframe`:
   *  - `string`: render produced a theme. Read the compiled CSS text
   *    from the VFS and post `{ cssText, fingerprint }`.
   *  - `null`: render succeeded with no theme intended. Post
   *    `{ cssText: null, fingerprint: null }` so the iframe drops its
   *    `<link data-q2-theme>` element.
   *  - `undefined`: render failed or pre-first-render. Skip the post
   *    entirely so the iframe keeps its last-good styling.
   *
   * Unlike `Q2PreviewIframe`, the CSS travels as **text**, not a blob
   * URL: blob URLs are scoped to the origin that minted them, so the
   * cross-origin sandboxed iframe could never fetch a parent-minted
   * one. The iframe mints its own blob URL from the text.
   */
  themeFingerprint?: string | null;
}

// The sandboxed preview is served from a separate origin (GitHub Pages) so the
// iframe gets real cross-origin isolation; see
// .github/workflows/github-pages.md. Set
// VITE_Q2_SANDBOXED_PREVIEW_URL to override (e.g.
// 'q2-sandboxed-preview/index.html' for the same-origin copy in public/).
const Q2_SANDBOXED_PREVIEW_URL = import.meta.env.VITE_Q2_SANDBOXED_PREVIEW_URL || 'https://quarto-dev.github.io/q2/';

/**
 * Iframe wrapper for the sandboxed (cross-origin) preview renderer.
 *
 * The iframe bundles the real q2-preview renderer (`@quarto/preview-renderer`
 * via the quarto-hub-sandboxed-preview project) and talks to this parent
 * exclusively over postMessage; assets are proxied through the iframe's
 * service worker (see `quarto-hub-sandboxed-preview/src/serviceWorker.ts`).
 */
export function Q2SandboxedPreviewIframe({
  astJson,
  currentFilePath,
  themeFingerprint,
}: Q2SandboxedPreviewIframeProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [iframeReady, setIframeReady] = useState(false);

  // Dedupe UPDATE_THEME posts; reset on IFRAME_READY (fresh iframe).
  const lastSentThemeFingerprintRef = useRef<string | null | undefined>(undefined);

  // Handle messages from the iframe
  // the requests are sent from `requestVFS` in `registerServiceWorker.ts` in the
  // `quarto-hub-sandboxed-preview` project.
  useEffect(() => {
    const handleMessage = async (event: MessageEvent) => {
      if (event.data.type === 'IFRAME_READY') {
        lastSentThemeFingerprintRef.current = undefined;
        setIframeReady(true)
      } else if (event.data.type === 'url' && event.data.path) {
        // Read from VFS and respond. `path` is the fully-resolved VFS path
        // extracted from the __q2_vfs__ proxy URL (the parent resolved it
        // against currentFilePath when it built the asset manifest, or the
        // theme-CSS rewriter resolved it against the artifact dir); `id`
        // correlates the response with the requesting fetch.
        const wasm = await import('wasm-quarto-hub-client');

        const isBinary = isBinaryPath(event.data.path);
        const resultJson = isBinary
          ? wasm.vfs_read_binary_file(event.data.path)
          : wasm.vfs_read_file(event.data.path);

        const result = JSON.parse(resultJson) as {
          success: boolean;
          content?: string;
          error?: string;
        };

        if (iframeRef.current?.contentWindow) {
          iframeRef.current.contentWindow.postMessage(
            {
              type: 'url_response',
              id: event.data.id,
              path: event.data.path,
              success: result.success,
              content: result.content,
              error: result.error,
              isBinary,
            },
            '*'
          );
        }
      }
    };

    window.addEventListener('message', handleMessage);
    return () => window.removeEventListener('message', handleMessage);
  }, []);

  // Proxy-URL asset manifest, rebuilt when the AST or document changes.
  // Cheap (no VFS reads — bytes are fetched on demand through the
  // service worker), but memoized so unrelated re-renders don't re-walk.
  const assetManifest = useMemo(
    () => buildProxyAssetManifest(astJson, currentFilePath),
    [astJson, currentFilePath],
  );

  // Send AST updates when iframe is ready. The manifest piggybacks on the
  // AST payload so an Image can never render before its manifest entry.
  useEffect(() => {
    if (!iframeReady || !iframeRef.current?.contentWindow) return;

    iframeRef.current.contentWindow.postMessage(
      {
        type: 'UPDATE_AST',
        payload: { astJson, currentFilePath, assetManifest },
      },
      '*'
    );

  }, [iframeReady, astJson, currentFilePath, assetManifest]);

  // Send theme CSS text when iframe is ready and fingerprint is known.
  useEffect(() => {
    if (!iframeReady || !iframeRef.current?.contentWindow) return;
    if (themeFingerprint === undefined) return;
    if (lastSentThemeFingerprintRef.current === themeFingerprint) return;

    let cssText: string | null = null;
    if (themeFingerprint !== null) {
      const result = vfsReadFile(DEFAULT_CSS_ARTIFACT_PATH);
      if (result.success && result.content) {
        cssText = result.content;
      }
    }

    iframeRef.current.contentWindow.postMessage(
      { type: 'UPDATE_THEME', cssText, fingerprint: themeFingerprint },
      '*'
    );
    lastSentThemeFingerprintRef.current = themeFingerprint;
  }, [iframeReady, themeFingerprint]);

  return (
    <iframe
      ref={iframeRef}
      src={Q2_SANDBOXED_PREVIEW_URL}
      title="q2-sandboxed-preview Renderer"
      sandbox="allow-scripts allow-same-origin"
      style={{
        width: '99%',
        height: '100%',
        border: 'none',
        display: 'block',
        // The sandboxed document paints no background of its own, so the
        // pane behind it shows through. Pin a light canvas: the document
        // content assumes one (default dark text), independent of the
        // editor chrome theme (.preview-pane follows the theme).
        background: '#fff',
      }}
    />
  );
}
