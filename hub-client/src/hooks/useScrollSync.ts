/**
 * React hook for bidirectional scroll synchronization between Monaco editor and preview iframe.
 *
 * Features:
 * - Editor → Preview: Cursor movement scrolls preview to corresponding content
 * - Preview → Editor: Scroll in preview scrolls editor viewport (without moving cursor)
 * - 50ms debounce to prevent jitter
 * - Graceful degradation when source locations unavailable
 *
 * Editor → Preview can be *deferred* (`deferToRender`): the cursor moves the
 * instant a keystroke lands, but fresh `data-loc` only reaches the preview DOM
 * once the async render commits. For the q2-preview path (whose iframe posts
 * `AST_RENDERED`), a cursor move during an edit defers its scroll until that
 * signal (`handleAstRendered`) — firing once, against the fresh DOM. The HTML
 * preview has no such signal, so it leaves `deferToRender` off and scrolls
 * immediately. `scrollToLineDeferred` lets replay drive the same deferred
 * mechanism with an explicit line instead of the cursor.
 */

import { useEffect, useRef, useCallback } from 'react';
import type { RefObject } from 'react';
import type * as Monaco from 'monaco-editor';

interface UseScrollSyncOptions {
  /** Reference to Monaco editor instance */
  editorRef: RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
  /** Function to scroll preview to a specific line (provided by DoubleBufferedIframe) */
  scrollPreviewToLine: (line: number) => void;
  /** Function to get the preview's scroll ratio (provided by DoubleBufferedIframe) */
  getPreviewScrollRatio: () => number | null;
  /** Whether scroll sync is enabled */
  enabled: boolean;
  /** Reference tracking whether editor has focus (to prevent feedback loop) */
  editorHasFocusRef: RefObject<boolean>;
  /**
   * Defer editor→preview scroll until the preview reports it committed a new
   * AST (`handleAstRendered`). True for q2-preview, whose iframe posts
   * `AST_RENDERED`; left false for the HTML preview, which has no such signal
   * and scrolls immediately on cursor move (its DOM is already current).
   */
  deferToRender?: boolean;
}

/**
 * Return type: callbacks for preview to call when scrolled or clicked
 */
interface UseScrollSyncReturn {
  handlePreviewScroll: () => void;
  handlePreviewClick: () => void;
  /**
   * Preview→editor click sync (click-to-editor-scroll): align `line` in the
   * editor to the same on-screen y as the clicked block, `hostY` (2026-08-22
   * click-align-editor-y plan). `hostY` omitted ⇒ top-align. Scroll-only —
   * calls `editor.setScrollTop` and nothing else. Deliberately narrower than
   * `syncPreviewToEditor`: in q2-preview the click that reaches this also
   * opens an inline editor *inside the preview*, so pulling focus to Monaco
   * (`setPosition` / `setSelection` / `focus()`) would break the gesture the
   * same click just started, and `setPosition` would additionally fire
   * `onDidChangeCursorPosition`, feeding editor→preview sync and bouncing.
   * Not focus-gated (an explicit click is unambiguous user intent, unlike
   * the scroll-ratio feedback loop `syncPreviewToEditor` guards against),
   * and not debounced (there's exactly one click, not a scroll stream).
   * Also, deliberately, not gated on `enabled` (decision A6): with the
   * scroll-sync toggle off, click-align is the only scroll coupling left in
   * either direction, which is how the feature is best evaluated — see A1h.
   * Do not add an `enabledRef.current` check here.
   */
  revealEditorLine: (line: number, hostY?: number) => void;
  /**
   * Call when the preview iframe reports it committed a new AST
   * (`AST_RENDERED`). Flushes any editor→preview scroll deferred while the
   * render was in flight — once, against the up-to-date DOM. A no-op when no
   * scroll is pending (so a collaborator's render never yanks).
   */
  handleAstRendered: () => void;
  /**
   * Scroll the preview to an explicit line, deferred to the next render the
   * same way cursor-driven sync is. Replay uses this so history scrubbing
   * shares the q2-preview scroll mechanism rather than a separate ratio path.
   * Programmatic, so unlike the cursor path it isn't gated on editor focus.
   */
  scrollToLineDeferred: (line: number) => void;
}

// A render that never reports back must not strand a deferred scroll: an edit
// that errored produces no `AST_RENDERED`, and an edit that doesn't change the
// AST produces no iframe re-render either. After this long we give up waiting
// and scroll against whatever DOM is current (the last-good one).
const RENDER_SETTLE_TIMEOUT_MS = 1000;

/**
 * Hook for bidirectional scroll synchronization between Monaco editor and preview iframe.
 * Returns callbacks that should be passed to the preview component.
 */
export function useScrollSync({
  editorRef,
  scrollPreviewToLine,
  getPreviewScrollRatio,
  enabled,
  editorHasFocusRef,
  deferToRender = false,
}: UseScrollSyncOptions): UseScrollSyncReturn {
  // Track if we're currently syncing to prevent feedback loops
  const isSyncingRef = useRef(false);

  // Debounce timers
  const editorDebounceRef = useRef<number | null>(null);
  const previewDebounceRef = useRef<number | null>(null);

  // A scroll is pending, awaiting flush. `pendingTargetRef` holds an explicit
  // target line (scrollToLineDeferred / replay); null means "use the cursor".
  const pendingScrollRef = useRef(false);
  const pendingTargetRef = useRef<number | null>(null);
  // An edit changed the content, so a render (→ AST_RENDERED) is expected; a
  // pending scroll defers until the fresh data-loc DOM exists. Only tracked
  // when deferToRender is on.
  const renderPendingRef = useRef(false);
  // Safety timer that bounds the above deferral (see RENDER_SETTLE_TIMEOUT_MS).
  const renderWaitRef = useRef<number | null>(null);

  // Latest option callbacks held in refs so the returned handlers keep a
  // stable identity. `handleAstRendered` feeds Q2PreviewIframe's message
  // listener deps; a churning identity re-subscribes that window listener and
  // can drop a postMessage racing the swap.
  const scrollPreviewToLineRef = useRef(scrollPreviewToLine);
  const getPreviewScrollRatioRef = useRef(getPreviewScrollRatio);
  const enabledRef = useRef(enabled);
  scrollPreviewToLineRef.current = scrollPreviewToLine;
  getPreviewScrollRatioRef.current = getPreviewScrollRatio;
  enabledRef.current = enabled;

  const clearRenderWait = useCallback(() => {
    if (renderWaitRef.current != null) {
      clearTimeout(renderWaitRef.current);
      renderWaitRef.current = null;
    }
  }, []);

  // Flush a pending editor→preview scroll. An explicit target
  // (scrollToLineDeferred, used by replay) is programmatic and not focus-gated;
  // a cursor-driven scroll is dropped if focus moved to the preview, which is
  // then the active surface (preview→editor).
  const flushPendingScroll = useCallback(() => {
    if (!pendingScrollRef.current) return;
    pendingScrollRef.current = false;
    clearRenderWait();

    const target = pendingTargetRef.current;
    pendingTargetRef.current = null;

    if (!enabledRef.current) return;
    const editor = editorRef.current;
    if (!editor) return;

    let line: number;
    if (target != null) {
      line = target;
    } else {
      if (!editorHasFocusRef.current) return;
      const position = editor.getPosition();
      if (!position) return;
      line = position.lineNumber;
    }

    isSyncingRef.current = true;
    scrollPreviewToLineRef.current(line);
    // Reset syncing flag after animation completes
    setTimeout(() => {
      isSyncingRef.current = false;
    }, 300);
  }, [clearRenderWait, editorRef, editorHasFocusRef]);

  // Preview → Editor sync (using scroll ratio matching)
  const syncPreviewToEditor = useCallback(() => {
    // Skip if disabled, already syncing, or editor has focus (prevents feedback loop)
    if (!enabledRef.current || isSyncingRef.current || editorHasFocusRef.current) return;

    const editor = editorRef.current;
    if (!editor) return;

    const scrollRatio = getPreviewScrollRatioRef.current();
    if (scrollRatio === null) return;

    // Apply same ratio to editor
    const editorScrollHeight = editor.getScrollHeight();
    const editorViewportHeight = editor.getLayoutInfo().height;
    const editorMaxScroll = editorScrollHeight - editorViewportHeight;

    const editorScrollTop = scrollRatio * editorMaxScroll;

    isSyncingRef.current = true;
    // Use smooth scrolling (ScrollType.Smooth = 1)
    editor.setScrollTop(editorScrollTop, 1);
    setTimeout(() => {
      isSyncingRef.current = false;
    }, 300); // Longer timeout to account for smooth animation
  }, [editorRef, editorHasFocusRef]);

  // Editor cursor + (q2 only) content listeners. A cursor move marks a pending
  // scroll; when deferToRender is on, a content change marks a render as
  // expected so the debounce defers to the post-render flush (one scroll,
  // fresh DOM). The HTML preview leaves deferToRender off and flushes on the
  // debounce, since its DOM is already current.
  useEffect(() => {
    if (!enabled) return;

    const editor = editorRef.current;
    if (!editor) return;

    const cursorDisposable = editor.onDidChangeCursorPosition(() => {
      pendingScrollRef.current = true;
      pendingTargetRef.current = null;
      if (editorDebounceRef.current) {
        clearTimeout(editorDebounceRef.current);
      }
      editorDebounceRef.current = window.setTimeout(() => {
        // An edit is in flight — the DOM is about to change. Let the
        // post-render flush (handleAstRendered) do the single scroll.
        if (deferToRender && renderPendingRef.current) return;
        flushPendingScroll();
      }, 50);
    });

    let contentDisposable: Monaco.IDisposable | undefined;
    if (deferToRender) {
      contentDisposable = editor.onDidChangeModelContent(() => {
        renderPendingRef.current = true;
        clearRenderWait();
        renderWaitRef.current = window.setTimeout(() => {
          renderWaitRef.current = null;
          renderPendingRef.current = false;
          flushPendingScroll();
        }, RENDER_SETTLE_TIMEOUT_MS);
      });
    }

    return () => {
      cursorDisposable.dispose();
      contentDisposable?.dispose();
      if (editorDebounceRef.current) {
        clearTimeout(editorDebounceRef.current);
      }
      clearRenderWait();
    };
  }, [enabled, editorRef, deferToRender, flushPendingScroll, clearRenderWait]);

  // Debounced preview scroll handler
  const handlePreviewScroll = useCallback(() => {
    // Debounce
    if (previewDebounceRef.current) {
      clearTimeout(previewDebounceRef.current);
    }
    previewDebounceRef.current = window.setTimeout(() => {
      syncPreviewToEditor();
    }, 50);
  }, [syncPreviewToEditor]);

  // Preview click handler (sync immediately, no debounce)
  const handlePreviewClick = useCallback(() => {
    syncPreviewToEditor();
  }, [syncPreviewToEditor]);

  // Preview→editor click sync (click-to-editor-scroll): align, not just
  // reveal — see the UseScrollSyncReturn doc comment for why (D1/D1b in the
  // original click-to-editor-scroll plan; alignment computation in the
  // 2026-08-22 click-align-editor-y plan). Not routed through
  // syncPreviewToEditor.
  //
  // On screen a source line sits at editorTop + (topForLine - scrollTop);
  // to land it at hostY we solve for scrollTop. Clamped to the editor's
  // scrollable range — near the start or end of the document the clamp
  // wins and the two panes won't line up exactly, which is a limit of the
  // geometry, not a bug. hostY omitted (decision A3) falls back to
  // top-aligning the line.
  //
  // Bracketed with isSyncingRef the same way flushPendingScroll and
  // syncPreviewToEditor already bracket their own scroll calls: a click that
  // activates an inline editor can produce a tiny, real reflow-driven scroll
  // in the preview a few ms later, which would otherwise reach
  // syncPreviewToEditor unguarded and overwrite this reveal with a
  // ratio-derived position ~50ms after it lands correctly (task-9 fix for
  // the race found in task-8's report).
  const revealEditorLine = useCallback((line: number, hostY?: number) => {
    const editor = editorRef.current;
    if (!editor) return;

    const topForLine = editor.getTopForLineNumber(line);
    const editorTop = editor.getDomNode()?.getBoundingClientRect().top;

    const desired =
      hostY !== undefined && editorTop !== undefined ? topForLine - (hostY - editorTop) : topForLine;

    const maxScroll = Math.max(0, editor.getScrollHeight() - editor.getLayoutInfo().height);
    const clamped = Math.min(Math.max(desired, 0), maxScroll);

    isSyncingRef.current = true;
    editor.setScrollTop(clamped, 1);
    setTimeout(() => {
      isSyncingRef.current = false;
    }, 300);
  }, [editorRef]);

  // The preview iframe committed a new AST: fresh data-loc DOM is in place,
  // so flush the deferred editor→preview scroll once.
  const handleAstRendered = useCallback(() => {
    renderPendingRef.current = false;
    flushPendingScroll();
  }, [flushPendingScroll]);

  // Scroll to an explicit line, deferred the same way the cursor path is.
  // If a render is in flight (deferToRender), the AST_RENDERED / safety flush
  // handles it; otherwise flush on the debounce against the current DOM.
  const scrollToLineDeferred = useCallback((line: number) => {
    if (!enabledRef.current) return;
    pendingScrollRef.current = true;
    pendingTargetRef.current = line;
    if (deferToRender && renderPendingRef.current) return;
    if (editorDebounceRef.current) {
      clearTimeout(editorDebounceRef.current);
    }
    editorDebounceRef.current = window.setTimeout(() => {
      flushPendingScroll();
    }, 50);
  }, [deferToRender, flushPendingScroll]);

  // Cleanup debounce timer on unmount
  useEffect(() => {
    return () => {
      if (previewDebounceRef.current) {
        clearTimeout(previewDebounceRef.current);
      }
    };
  }, []);

  return {
    handlePreviewScroll,
    handlePreviewClick,
    revealEditorLine,
    handleAstRendered,
    scrollToLineDeferred,
  };
}
