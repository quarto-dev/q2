import { useEffect, useImperativeHandle, useMemo, useRef, useState } from 'react';
import type { Ref } from 'react';
import { vfsReadFile } from '@quarto/preview-runtime';
import { DEFAULT_CSS_ARTIFACT_PATH } from '../types/artifactPaths';
import { buildAssetManifest, type ManifestCacheEntry } from '../q2-preview/assetWalker';
import { scrollIframeToLine, getIframeScrollRatio, lineForClickTarget } from './scrollSyncDom';

/**
 * Imperative scroll-sync handle, parallel to `MorphIframeHandle`. The
 * q2-preview iframe is same-origin (`allow-same-origin`), so the parent
 * reads `contentDocument` / `contentWindow` directly rather than going
 * through postMessage. Editor→preview uses `data-loc` attributes the
 * q2-preview leaves stamp via `dataLocProps`; preview→editor uses the
 * raw scroll ratio.
 */
export interface Q2PreviewIframeHandle {
  scrollToLine: (line: number) => void;
  getScrollRatio: () => number | null;
}

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
  /**
   * Pre-pipeline (untransformed) AST JSON shipped in lockstep with
   * `astJson` + `renderedContent` (same compound-state generation).
   * Forwarded to the iframe for the structural editability gate (Plan 2a).
   */
  untransformedAstJson?: string | null;
  /**
   * Reactji-authorship demo (2026-05-25 plan): current viewer's
   * Automerge actor id, forwarded into the iframe so user TSX can
   * compare `actor === me` against `useNodeAttribution(node).actor`.
   * `null` when the producer has no actor yet (project initialising,
   * non-Automerge document). Piggybacks on the `UPDATE_AST` payload
   * because the value is stable per device — coupling to AST cadence
   * is fine. Long-term home is `astContext.currentActor` (Plan 5
   * follow-up).
   */
  currentActor?: string | null;
  /**
   * Globally disable the edit surface inside the iframe (bd-ov4gqk3m).
   * Set by read-only hosts — the q2-preview SPA when the server
   * reports `allowEdit: false`. Absent/false ⇒ editable (hub-client's
   * existing behavior).
   */
  editingDisabled?: boolean;
  /**
   * Comment-bubble display mode (host three-way toggle: expand / show /
   * hide). Forwarded unchanged in the UPDATE_AST payload. Absent ⇒ 'show'.
   */
  commentsMode?: 'expand' | 'show' | 'hide';
  /**
   * P3.2: nesting-cursor mode for nested blocks. When true, the iframe's
   * PreviewContext exposes nesting-cursor behaviour. Default-off
   * (undefined/false). Forwarded unchanged in the UPDATE_AST payload.
   */
  unlockNestingCursor?: boolean;
  /**
   * Phase 1a (bd-sjb4pzx8): opt-in rich-text (tiptap) block editor. Forwarded
   * unchanged in the UPDATE_AST payload into `PreviewContext.richText`.
   */
  richText?: boolean;
  /**
   * P3.2: per-siKey clean QMD buffers for nested blocks, produced by
   * the host's `regenerateNestedBuffers` call (gated on
   * `unlockNestingCursor`). Forwarded unchanged in the UPDATE_AST payload
   * into `PreviewContext.nestedEditBuffers`.
   */
  nestedEditBuffers?: Record<string, string>;
  /**
   * Controlled current slide index for `format: revealjs` decks
   * (bd-mwbsdmel). Driven by the editor's cursor→slide mapping. When it
   * changes, the wrapper posts `SET_SLIDE` to the iframe, which navigates
   * the reveal deck imperatively (no AST re-render — cursor moves are far
   * more frequent than edits). Ignored by non-slide previews (the iframe
   * has no slide navigator registered).
   */
  currentSlideIndex?: number;
  /**
   * Slide-change callback for revealjs decks (bd-mwbsdmel). Invoked with
   * the new horizontal slide index when the deck navigates inside the
   * iframe (arrow keys, controls), via a `SLIDE_CHANGED` message. Lets the
   * editor keep its slide state (and thumbnail highlight) in sync with
   * in-deck navigation.
   */
  onSlideChange?: (slideIndex: number) => void;
  /** Scroll-sync handle (scrollToLine / getScrollRatio). */
  scrollHandleRef?: Ref<Q2PreviewIframeHandle>;
  /** Called when the preview iframe is scrolled (preview→editor sync). */
  onScroll?: () => void;
  /**
   * Called with the resolved source line when a capture-phase `pointerup`
   * on the iframe document resolves one via `lineForClickTarget`
   * (preview→editor click sync). Not called at all when the click doesn't
   * resolve a line (see `lineForClickTarget`'s null cases). The second
   * argument, `hostY`, is the clicked block's top edge in HOST-PAGE
   * coordinates (see the `pointerup` handler below) — used by
   * `useScrollSync.revealEditorLine` to align that source line to the same
   * on-screen y, not just reveal it (2026-08-22 click-align-editor-y plan).
   */
  onClickAtLine?: (line: number, hostY?: number) => void;
  /**
   * Called after the iframe commits a new AST (`AST_RENDERED`), so the host
   * can re-run editor→preview scroll sync against the fresh `data-loc` DOM.
   */
  onAstRendered?: () => void;
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
  untransformedAstJson,
  currentActor,
  editingDisabled,
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
}: Q2PreviewIframeProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [iframeReady, setIframeReady] = useState(false);

  // Last slide index posted to the iframe, so we dedupe SET_SLIDE sends
  // and don't echo a slide back to the deck it just reported (which would
  // otherwise loop: cursor → SET_SLIDE → slidechanged → SLIDE_CHANGED →
  // editor state → SET_SLIDE …). Reset on IFRAME_READY (fresh iframe).
  const lastSentSlideRef = useRef<number | undefined>(undefined);

  // Scroll-sync handle. Same-origin access to the iframe document lets us
  // reuse the exact shared scroll helpers `MorphIframe` uses for the HTML
  // preview.
  useImperativeHandle(
    scrollHandleRef,
    () => ({
      scrollToLine: (line: number) => scrollIframeToLine(iframeRef.current, line),
      getScrollRatio: () => getIframeScrollRatio(iframeRef.current),
    }),
    [],
  );

  // Forward iframe scroll / click to the parent for preview→editor sync.
  // Attached once the iframe document exists (IFRAME_READY); the q2-preview
  // app re-renders its body via React on each UPDATE_AST but never reloads
  // the iframe, so the contentWindow/document — and these listeners —
  // persist across edits.
  //
  // The click side listens for `pointerup` in the CAPTURE phase, not
  // `click` in the bubble phase. q2-preview's block-edit activation also
  // fires on `pointerup` (bubble phase) and replaces the clicked element's
  // subtree with `#q2-active-edit-region` — Chromium then dispatches no
  // `click` at all, because the `pointerup` target was detached from the
  // document during the event. Capture-phase `pointerup` runs before that
  // bubble-phase activation handler, while the target is still attached and
  // its nearest `[data-loc]` ancestor is still resolvable.
  useEffect(() => {
    if (!iframeReady) return;
    const iframe = iframeRef.current;
    const win = iframe?.contentWindow;
    const doc = iframe?.contentDocument;
    if (!win || !doc) return;

    const handleScroll = () => onScroll?.();
    const handlePointerUp = (e: Event) => {
      const line = lineForClickTarget(e.target);
      if (line === null) return;
      // Report the clicked block's top edge in HOST-PAGE coordinates, so
      // the editor can align that line to the same on-screen y as the
      // preview's editing pane (decision A2, 2026-08-22 plan): the block's
      // rect is in the IFRAME's viewport, so the iframe element's own top
      // in the host page has to be added. The edit region doesn't exist yet
      // at capture-phase pointerup — it replaces the clicked block in
      // place, so the block's top *is* the pane's top.
      const target = e.target as { closest?: Element['closest'] } | null;
      const block = target?.closest?.('[data-loc]');
      const blockTop = block?.getBoundingClientRect().top;
      const iframeTop = iframe?.getBoundingClientRect().top;
      const hostY =
        blockTop !== undefined && iframeTop !== undefined ? blockTop + iframeTop : undefined;
      onClickAtLine?.(line, hostY);
    };

    win.addEventListener('scroll', handleScroll, { passive: true });
    doc.addEventListener('pointerup', handlePointerUp, true);
    return () => {
      win.removeEventListener('scroll', handleScroll);
      doc.removeEventListener('pointerup', handlePointerUp, true);
    };
  }, [iframeReady, onScroll, onClickAtLine]);

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
        lastSentSlideRef.current = undefined;
        if (currentThemeBlobUrlRef.current) {
          URL.revokeObjectURL(currentThemeBlobUrlRef.current);
          currentThemeBlobUrlRef.current = null;
        }
      } else if (event.data.type === 'NAVIGATE_TO_DOCUMENT') {
        onNavigateToDocument?.(event.data.path, event.data.anchor);
      } else if (event.data.type === 'SET_AST') {
        setAst(event.data.ast);
      } else if (event.data.type === 'SLIDE_CHANGED') {
        // The deck navigated inside the iframe (arrows/controls). Record
        // it as already-synced so the resulting editor state change does
        // not bounce back as a redundant SET_SLIDE, then report it up.
        lastSentSlideRef.current = event.data.index;
        onSlideChange?.(event.data.index);
      } else if (event.data.type === 'AST_RENDERED') {
        // The iframe committed a new AST and laid out; the fresh data-loc
        // DOM is now in place, so re-run editor→preview scroll sync.
        onAstRendered?.();
      }
    };

    window.addEventListener('message', handleMessage);
    return () => window.removeEventListener('message', handleMessage);
  }, [onNavigateToDocument, setAst, onSlideChange, onAstRendered]);

  // Post the controlled slide index to the iframe when it changes. The
  // iframe navigates the reveal deck imperatively (RevealNavSync), so
  // this never triggers an AST re-render. Deduped against the last sent
  // value so an in-deck navigation echoed back through the editor's
  // slide state does not re-post the same index.
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
          // Plan 2a: pre-pipeline AST for the structural editability
          // gate. Shipped in lockstep with astJson + renderedContent
          // (same compound-state generation) so they can never skew.
          untransformedAstJson,
          // Reactji-authorship demo (2026-05-25 plan): viewer's
          // Automerge actor id, threaded through to user TSX.
          currentActor,
          // bd-ov4gqk3m: read-only hosts disable the edit surface.
          editingDisabled,
          // Comment-bubble mode (expand / show / hide).
          commentsMode,
          // P3.2: nesting-cursor mode + per-key nested buffers.
          unlockNestingCursor,
          // Phase 1a (bd-sjb4pzx8): opt-in rich-text editor.
          richText,
          nestedEditBuffers,
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
    untransformedAstJson,
    currentActor,
    editingDisabled,
    commentsMode,
    unlockNestingCursor,
    richText,
    nestedEditBuffers,
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
