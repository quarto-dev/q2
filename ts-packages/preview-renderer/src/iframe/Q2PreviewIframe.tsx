import { useEffect, useMemo, useRef, useState } from 'react';
import { vfsReadFile } from '@quarto/preview-runtime';
import { DEFAULT_CSS_ARTIFACT_PATH } from '../types/artifactPaths';
import { buildAssetManifest, type ManifestCacheEntry } from '../q2-preview/assetWalker';

interface Q2PreviewIframeProps {
  astJson: string;
  currentFilePath: string;
  onNavigateToDocument?: (path: string, anchor: string | null) => void;
  setAst: (newAst: any) => void;
  customComponentsCode?: Record<string, string>;
  /**
   * Three-way theme fingerprint (Plan 2A item 11).
   *  - `string`: render produced a theme. Read VFS bytes, mint a blob
   *    URL, post `{ cssUrl, fingerprint }`.
   *  - `null`: render succeeded with no theme intended. Post
   *    `{ cssUrl: null, fingerprint: null }` so the iframe drops its
   *    `<link data-q2-theme>` element.
   *  - `undefined`: render failed or pre-first-render. **Skip the post
   *    entirely** so the iframe keeps its last-good styling — a
   *    transient YAML parse error shouldn't strip Bootstrap from the
   *    user's view.
   */
  themeFingerprint?: string | null;
  /**
   * Phase F.1 (bd-kw93.14): list of project file paths (no leading
   * slash) forwarded to the iframe's link handler so artifact-rooted
   * `.html` clicks can be reverse-mapped to the source `.qmd` for
   * the SPA-style navigation flow.
   */
  projectFilePaths?: readonly string[];
  /**
   * Phase F.1 (bd-kw93.14): anchor (without `#`) to scroll to AFTER
   * the iframe commits the current AST. Set by `PreviewApp` when the
   * user clicks a cross-page link with `#frag`, or when popstate
   * restores a hash.
   *
   * Paired with `pendingAnchorEpoch`: the iframe scrolls only when
   * the epoch changes since the last scroll, so subsequent edits
   * (which re-post UPDATE_AST with the same anchor + same epoch)
   * don't jerk the user back. Each user-driven nav event bumps the
   * epoch in `PreviewApp`.
   */
  pendingAnchor?: string | null;
  pendingAnchorEpoch?: number;
  /**
   * The QMD source text that was used to produce the current render
   * generation (Task 3 / block-editing plan). The byte offsets in
   * `astJson` belong to this snapshot. Forwarded to the iframe in the
   * UPDATE_AST payload as `content` so the iframe can slice source
   * bytes without skew.
   *
   * Optional so the component degrades gracefully before entry.tsx is
   * updated to consume it.
   */
  renderedContent?: string;
}

/**
 * Wrapper component that renders q2-preview's `<Ast>` in a sandboxed
 * iframe. Parallel to `Q2DebugIframe` with two additions:
 *
 *  1. Blob-URL theme transport. The iframe is sandboxed and does not
 *     initialize WASM, so it cannot call `vfsReadFile` directly. The
 *     parent (this component) reads the compiled theme CSS bytes,
 *     wraps them in a `Blob`, mints a blob URL via
 *     `URL.createObjectURL`, and posts the URL string to the iframe
 *     in an `UPDATE_THEME` message. Iframe consumes the URL via
 *     `<link rel="stylesheet" href={cssUrl}>`. No bytes ever ride on
 *     the postMessage channel.
 *
 *  2. Blob-URL revocation lifecycle. Three triggers: (a) replacement
 *     when the fingerprint changes and a new URL is minted, (b)
 *     `IFRAME_READY` (a fresh iframe means any prior URL we held is
 *     no longer referenced), (c) iframe unmount.
 */
export function Q2PreviewIframe({
  astJson,
  currentFilePath,
  onNavigateToDocument,
  setAst,
  customComponentsCode,
  themeFingerprint,
  projectFilePaths,
  pendingAnchor,
  pendingAnchorEpoch,
  renderedContent,
}: Q2PreviewIframeProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [iframeReady, setIframeReady] = useState(false);

  // Track the last fingerprint we posted and the current outstanding
  // blob URL so we can dedupe re-sends and revoke replaced URLs.
  const lastSentThemeFingerprintRef = useRef<string | null | undefined>(
    undefined,
  );
  const currentThemeBlobUrlRef = useRef<string | null>(null);

  // Asset-manifest cache. Owned by this component for the iframe's
  // lifetime; entries are revoked individually as image content
  // changes (per `buildAssetManifest`'s eviction logic), and the whole
  // cache is drained on unmount.
  const assetCacheRef = useRef<Map<string, ManifestCacheEntry>>(new Map());

  // Cleanup any outstanding blob URL on unmount. Drains both the theme
  // URL and the asset cache.
  useEffect(() => {
    const assetCache = assetCacheRef.current;
    return () => {
      if (currentThemeBlobUrlRef.current) {
        URL.revokeObjectURL(currentThemeBlobUrlRef.current);
        currentThemeBlobUrlRef.current = null;
      }
      for (const entry of assetCache.values()) {
        URL.revokeObjectURL(entry.url);
      }
      assetCache.clear();
    };
  }, []);

  // Build the asset manifest once per (astJson, currentFilePath) pair.
  // Memoization keeps the walker from re-running on unrelated re-renders
  // (e.g. theme changes don't disturb the manifest). The walker mutates
  // `assetCacheRef.current` in place, evicting / minting blob URLs as
  // image content changes between renders.
  const assetManifest = useMemo(
    () => buildAssetManifest(astJson, currentFilePath, assetCacheRef.current).manifest,
    [astJson, currentFilePath],
  );

  // Handle messages from the iframe
  useEffect(() => {
    const handleMessage = (event: MessageEvent) => {
      // In production, verify event.origin for security
      if (event.data.type === 'IFRAME_READY') {
        setIframeReady(true);
        // Iframe restart — invalidate dedup state. Any prior blob URL
        // we held is no longer referenced by the new iframe; revoke it.
        lastSentThemeFingerprintRef.current = undefined;
        if (currentThemeBlobUrlRef.current) {
          URL.revokeObjectURL(currentThemeBlobUrlRef.current);
          currentThemeBlobUrlRef.current = null;
        }
      } else if (event.data.type === 'NAVIGATE_TO_DOCUMENT') {
        onNavigateToDocument?.(event.data.path, event.data.anchor);
      } else if (event.data.type === 'SET_AST') {
        setAst(event.data.ast);
      }
    };

    window.addEventListener('message', handleMessage);
    return () => window.removeEventListener('message', handleMessage);
  }, [onNavigateToDocument, setAst]);

  // Send custom components code when iframe is ready (only once or when it changes)
  useEffect(() => {
    if (!iframeReady || !iframeRef.current?.contentWindow) return;

    if (customComponentsCode) {
      iframeRef.current.contentWindow.postMessage(
        {
          type: 'LOAD_CUSTOM_COMPONENTS',
          componentsCode: customComponentsCode,
        },
        '*',
      );
    }
  }, [iframeReady, customComponentsCode]);

  // Send AST updates when iframe is ready. The manifest piggybacks on
  // the AST payload — manifest and AST are derived from the same input
  // (manifest is built from AST contents), and shipping them together
  // guarantees ordering: an Image rendered before its manifest entry
  // arrived would briefly resolve to the original URL (broken).
  useEffect(() => {
    if (!iframeReady || !iframeRef.current?.contentWindow) return;

    iframeRef.current.contentWindow.postMessage(
      {
        type: 'UPDATE_AST',
        payload: {
          astJson,
          currentFilePath,
          assetManifest,
          // Phase F.1 (bd-kw93.14): the iframe link handler reads
          // `projectFilePaths` from this payload to recognize
          // artifact-rooted `.html` hrefs as project documents.
          projectFilePaths,
          // Phase F.1: scroll target after React commits the new AST.
          // Cross-page nav posts `{ ..., pendingAnchor: 'sec' }`; the
          // iframe scrolls in a useEffect once the new DOM exists.
          pendingAnchor,
          pendingAnchorEpoch,
          // Task 3 (block-editing): the QMD source snapshot whose byte
          // offsets correspond to astJson. The iframe uses this to
          // slice source ranges for inline editing without skew.
          renderedContent,
        },
      },
      '*',
    );
  }, [
    iframeReady,
    astJson,
    currentFilePath,
    assetManifest,
    projectFilePaths,
    pendingAnchor,
    pendingAnchorEpoch,
    renderedContent,
  ]);

  // Send theme CSS when iframe is ready and fingerprint is known.
  // Three-way semantics:
  //   - undefined ⇒ skip post entirely; last-good CSS persists.
  //   - null      ⇒ post explicit clear; iframe drops its <link>.
  //   - string    ⇒ read VFS bytes, mint blob URL, post URL.
  useEffect(() => {
    if (!iframeReady || !iframeRef.current?.contentWindow) return;
    if (themeFingerprint === undefined) return;
    if (lastSentThemeFingerprintRef.current === themeFingerprint) return;

    // Replace the prior blob URL (if any). The iframe is about to
    // swap its <link href>; the browser internally retains the old
    // bytes during the transition, so revocation immediately after
    // posting is safe.
    if (currentThemeBlobUrlRef.current) {
      URL.revokeObjectURL(currentThemeBlobUrlRef.current);
      currentThemeBlobUrlRef.current = null;
    }

    let cssUrl: string | null = null;
    if (themeFingerprint !== null) {
      const result = vfsReadFile(DEFAULT_CSS_ARTIFACT_PATH);
      if (result.success && result.content) {
        const blob = new Blob([result.content], { type: 'text/css' });
        cssUrl = URL.createObjectURL(blob);
        currentThemeBlobUrlRef.current = cssUrl;
      }
    }

    iframeRef.current.contentWindow.postMessage(
      { type: 'UPDATE_THEME', cssUrl, fingerprint: themeFingerprint },
      '*',
    );
    lastSentThemeFingerprintRef.current = themeFingerprint;
  }, [iframeReady, themeFingerprint]);

  return (
    <iframe
      ref={iframeRef}
      src="q2-preview.html"
      title="q2-preview Renderer"
      sandbox="allow-scripts allow-same-origin"
      style={{
        width: '100%',
        height: '100%',
        border: 'none',
        display: 'block',
      }}
    />
  );
}
