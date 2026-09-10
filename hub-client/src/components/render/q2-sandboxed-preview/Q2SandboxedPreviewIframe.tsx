import { useEffect, useImperativeHandle, useMemo, useRef, useState } from 'react';
import type { Ref } from 'react';
import { vfsReadFile } from '@quarto/preview-runtime';
import { DEFAULT_CSS_ARTIFACT_PATH } from '@quarto/preview-renderer/types/artifactPaths';
import type { Q2PreviewIframeHandle } from '@quarto/preview-renderer/iframe/Q2PreviewIframe';
import { isBinaryPath } from '../../../../quarto-hub-sandboxed-preview/src/assetPolicy';
import { buildProxyAssetManifest } from './proxyAssetManifest';

interface Q2SandboxedPreviewIframeProps {
  astJson: string;
  currentFilePath: string;
  onNavigateToDocument?: (path: string, anchor: string | null) => void;
  setAst: (newAst: any) => void;
  customComponentsCode?: Record<string, string>;
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
  projectFilePaths?: readonly string[];
  pendingAnchor?: string | null;
  pendingAnchorEpoch?: number;
  renderedContent?: string;
  untransformedAstJson?: string | null;
  currentActor?: string | null;
  commentsMode?: 'expand' | 'show' | 'hide';
  unlockNestingCursor?: boolean;
  richText?: boolean;
  nestedEditBuffers?: Record<string, string>;
  currentSlideIndex?: number;
  onSlideChange?: (slideIndex: number) => void;
  /**
   * Scroll-sync handle, same interface as `Q2PreviewIframe`'s — but
   * implemented over postMessage: `scrollToLine` posts SCROLL_TO_LINE
   * (the iframe does the data-loc lookup itself), and `getScrollRatio`
   * returns the last ratio the iframe reported via PREVIEW_SCROLLED
   * (null before the first report).
   */
  scrollHandleRef?: Ref<Q2PreviewIframeHandle>;
  onScroll?: () => void;
  /**
   * Preview→editor click sync. The iframe reports the clicked block's
   * top edge in ITS viewport coordinates (`iframeY`); this component
   * adds its own bounding rect's top to produce `hostY` in host-page
   * coordinates — the piece the iframe cannot know.
   */
  onClickAtLine?: (line: number, hostY?: number) => void;
  onAstRendered?: () => void;
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
 * Feature parity with `Q2PreviewIframe`, restated for a frame the parent
 * cannot reach into:
 *  - the renderer bundle is the same `@quarto/preview-renderer` code
 *    (see quarto-hub-sandboxed-preview/src/entry.tsx);
 *  - assets are proxied through the iframe's service worker
 *    (`__q2_vfs__` namespace) instead of parent-minted blob URLs;
 *  - theme CSS travels as text instead of a blob URL;
 *  - scroll sync and click-to-line travel as postMessage
 *    (SCROLL_TO_LINE / PREVIEW_SCROLLED / CLICK_AT_LINE) instead of
 *    direct contentDocument reads.
 */
export function Q2SandboxedPreviewIframe({
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
  untransformedAstJson,
  currentActor,
  commentsMode,
  unlockNestingCursor,
  richText,
  nestedEditBuffers,
  currentSlideIndex,
  onSlideChange,
  scrollHandleRef,
  onScroll,
  onClickAtLine,
  onAstRendered,
}: Q2SandboxedPreviewIframeProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [iframeReady, setIframeReady] = useState(false);

  // Dedupe UPDATE_THEME posts; reset on IFRAME_READY (fresh iframe).
  const lastSentThemeFingerprintRef = useRef<string | null | undefined>(undefined);

  // SET_SLIDE dedup, reset on IFRAME_READY — same echo-loop guard as
  // Q2PreviewIframe (cursor → SET_SLIDE → slidechanged → SLIDE_CHANGED →
  // editor state → SET_SLIDE …).
  const lastSentSlideRef = useRef<number | undefined>(undefined);

  // Last scroll ratio the iframe reported (PREVIEW_SCROLLED). The handle's
  // getScrollRatio must answer synchronously, so it reads this cache.
  const lastScrollRatioRef = useRef<number | null>(null);

  useImperativeHandle(
    scrollHandleRef,
    () => ({
      scrollToLine: (line: number) => {
        iframeRef.current?.contentWindow?.postMessage({ type: 'SCROLL_TO_LINE', line }, '*');
      },
      getScrollRatio: () => lastScrollRatioRef.current,
    }),
    [],
  );

  // Handle messages from the iframe. `url` requests come from `requestVFS`
  // in `registerServiceWorker.ts` in the `quarto-hub-sandboxed-preview`
  // project.
  useEffect(() => {
    const handleMessage = async (event: MessageEvent) => {
      if (event.data.type === 'IFRAME_READY') {
        lastSentThemeFingerprintRef.current = undefined;
        lastSentSlideRef.current = undefined;
        lastScrollRatioRef.current = null;
        setIframeReady(true)
      } else if (event.data.type === 'NAVIGATE_TO_DOCUMENT') {
        onNavigateToDocument?.(event.data.path, event.data.anchor);
      } else if (event.data.type === 'SET_AST') {
        setAst(event.data.ast);
      } else if (event.data.type === 'SLIDE_CHANGED') {
        // The deck navigated inside the iframe; record as already-synced
        // so the resulting editor state change does not bounce back.
        lastSentSlideRef.current = event.data.index;
        onSlideChange?.(event.data.index);
      } else if (event.data.type === 'AST_RENDERED') {
        onAstRendered?.();
      } else if (event.data.type === 'PREVIEW_SCROLLED') {
        lastScrollRatioRef.current = event.data.ratio;
        onScroll?.();
      } else if (event.data.type === 'CLICK_AT_LINE') {
        const iframeTop = iframeRef.current?.getBoundingClientRect().top;
        const hostY =
          typeof event.data.iframeY === 'number' && iframeTop !== undefined
            ? event.data.iframeY + iframeTop
            : undefined;
        onClickAtLine?.(event.data.line, hostY);
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
  }, [onNavigateToDocument, setAst, onSlideChange, onAstRendered, onScroll, onClickAtLine]);

  // Post the controlled slide index when it changes, deduped against the
  // last sent/reported value.
  useEffect(() => {
    if (!iframeReady || !iframeRef.current?.contentWindow) return;
    if (currentSlideIndex === undefined) return;
    if (lastSentSlideRef.current === currentSlideIndex) return;
    lastSentSlideRef.current = currentSlideIndex;
    iframeRef.current.contentWindow.postMessage(
      { type: 'SET_SLIDE', index: currentSlideIndex },
      '*',
    );
  }, [iframeReady, currentSlideIndex]);

  // Send custom components code when iframe is ready (or when it changes).
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
        payload: {
          astJson,
          currentFilePath,
          assetManifest,
          projectFilePaths,
          pendingAnchor,
          pendingAnchorEpoch,
          renderedContent,
          untransformedAstJson,
          currentActor,
          commentsMode,
          unlockNestingCursor,
          richText,
          nestedEditBuffers,
        },
      },
      '*'
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
    untransformedAstJson,
    currentActor,
    commentsMode,
    unlockNestingCursor,
    richText,
    nestedEditBuffers,
  ]);

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
