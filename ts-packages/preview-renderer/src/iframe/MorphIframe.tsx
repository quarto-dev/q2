import { useRef, useEffect, useCallback, useImperativeHandle, useState } from 'react';
import type { Ref } from 'react';
import morphdom from 'morphdom';
import { postProcessIframe } from '../utils/iframePostProcessor';
import {
  parseDataLoc,
  scrollIframeToLine,
  getIframeScrollRatio,
  lineForClickTarget,
  type SourceLocation,
} from './scrollSyncDom';

// Methods exposed via ref
export interface MorphIframeHandle {
  scrollToLine: (line: number) => void;
  getScrollRatio: () => number | null;
  setScrollRatio: (ratio: number) => void;
  setSelection: (startPos: SourceLocation, endPos: SourceLocation) => void;
  clearSelection: () => void;
}

interface MorphIframeProps {
  // HTML content to render - component handles morphing automatically
  html: string;
  // Current file path for resolving relative links
  currentFilePath: string;
  // Project file paths (no leading slash). Used by the iframe
  // post-processor to reverse-map artifact-rooted .html links
  // back to source .qmd files for cross-doc click navigation
  // (bd-lnd3).
  projectFilePaths?: readonly string[];

  qmdContent: string;
  // Callback when user navigates to a different document (with optional anchor)
  // Parent (Preview) handles file lookup and switching
  onNavigateToDocument: (targetPath: string, anchor: string | null) => void;
  // Optional callback when preview is scrolled
  onScroll?: () => void;
  // Optional callback when preview is clicked
  onClick?: () => void;
  // Optional callback when selection changes in preview. The third
  // argument, `hostY`, is the anchor span's top edge in HOST-PAGE
  // coordinates (see `handleSelectionChange` below) — used by
  // `useScrollSync.revealEditorLine` to align the editor to the same
  // on-screen y as the selected text, rather than merely reveal it
  // (2026-08-22 click-align-editor-y plan, Phase 2).
  onSelectionChange?: (
    startPos: SourceLocation | null,
    endPos: SourceLocation | null,
    hostY?: number,
  ) => void;
  // Ref to expose imperative methods
  ref: Ref<MorphIframeHandle>;
}

// The scroll-sync handle methods (`scrollIframeToLine`, `getIframeScrollRatio`)
// and `SourceLocation`/`parseDataLoc` are shared with `Q2PreviewIframe` via
// `./scrollSyncDom`. The position-comparison + offset helpers below are
// selection-sync specific and stay local.

/**
 * Check if a position (line, col) is within or after the start of a data-loc range.
 */
function isPositionAfterOrAt(
  targetLine: number,
  targetCol: number,
  startLine: number,
  startCol: number
): boolean {
  if (targetLine > startLine) return true;
  if (targetLine === startLine && targetCol >= startCol) return true;
  return false;
}

/**
 * Check if a position (line, col) is within or before the end of a data-loc range.
 */
function isPositionBeforeOrAt(
  targetLine: number,
  targetCol: number,
  endLine: number,
  endCol: number
): boolean {
  if (targetLine < endLine) return true;
  if (targetLine === endLine && targetCol <= endCol) return true;
  return false;
}

/**
 * Convert (row, col) position to character offset from start of text.
 *
 * @param text - The source text
 * @param row - 1-based row number
 * @param col - 1-based column number
 * @returns Character offset from start of text, or null if position is out of bounds
 */
function rowAndColToOffset(
  text: string,
  row: number,
  col: number
): number | null {
  const lines = text.split('\n');

  // Validate input position
  if (row < 1 || row > lines.length) return null;
  if (col < 1 || col > lines[row - 1].length + 1) return null;

  // Calculate character offset from start of text
  let charOffset = 0;
  for (let i = 0; i < row - 1; i++) {
    charOffset += lines[i].length + 1; // +1 for newline
  }
  charOffset += col - 1;

  return charOffset;
}

/**
 * Convert character offset to (row, col) position.
 *
 * @param text - The source text
 * @param offset - Character offset from start of text
 * @returns (row, col) position (1-based), or null if offset is out of bounds
 */
function offsetToRowAndCol(
  text: string,
  offset: number
): { row: number, col: number } | null {
  // Validate offset is within bounds
  if (offset < 0 || offset > text.length) return null;

  const lines = text.split('\n');
  let currentOffset = 0;

  for (let i = 0; i < lines.length; i++) {
    const lineLength = lines[i].length;
    const lineEnd = currentOffset + lineLength;

    if (offset <= lineEnd) {
      return {
        row: i + 1,
        col: offset - currentOffset + 1
      };
    }

    currentOffset = lineEnd + 1; // +1 for newline
  }

  // Should not reach here if bounds check passed
  return null;
}

/**
 * Add a character offset to a position (row, col) in a string.
 *
 * @param text - The source text
 * @param row - 1-based row number
 * @param col - 1-based column number
 * @param offset - Number of characters to add (can be negative)
 * @returns New (row, col) position after applying offset, or null if out of bounds
 */
function addOffsetToPosition(
  text: string,
  row: number,
  col: number,
  offset: number
): { row: number, col: number } | null {
  const charOffset = rowAndColToOffset(text, row, col);
  if (charOffset === null) return null;

  return offsetToRowAndCol(text, charOffset + offset);
}

/**
 * Morph-based iframe component for seamless updates.
 *
 * Uses morphdom to update the iframe's content in-place, preserving:
 * - Scroll position
 * - DOM state (expanded/collapsed elements, etc.)
 * - Better performance for small changes
 *
 * When new HTML arrives:
 * 1. Saves current scroll position
 * 2. Uses morphdom to morph the iframe's document into the new HTML
 * 3. Post-processes the updated content (CSS, link handlers, etc.)
 * 4. Restores scroll position
 */
function MorphIframe({
  html,
  currentFilePath,
  projectFilePaths,
  qmdContent,
  onNavigateToDocument,
  onScroll,
  onClick,
  onSelectionChange,
  ref,
}: MorphIframeProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const isInitializedRef = useRef(false);
  // Flips true once the `srcdoc` load settles. The initial load replaces the
  // iframe's contentWindow/contentDocument asynchronously, so the scroll /
  // click / selectionchange listeners must (re)attach to the *settled*
  // document — not the pre-load one present at mount. Parallels
  // Q2PreviewIframe's `iframeReady` gate.
  const [documentReady, setDocumentReady] = useState(false);

  // Scroll the preview to an anchor element
  const scrollToAnchor = useCallback((anchor: string) => {
    const iframe = iframeRef.current;
    const doc = iframe?.contentDocument;
    if (!doc) return;

    const element = doc.getElementById(anchor);
    if (element) {
      element.scrollIntoView({ behavior: 'instant', block: 'start' });
    }
  }, []);

  // Handler for .qmd link clicks and anchor clicks in the preview
  const handleQmdLinkClick = useCallback(
    (arg: { path: string, anchor: string | null } | { anchor: string }) => {
      if ('path' in arg) {
        onNavigateToDocument(arg.path, arg.anchor);
      } else {
        scrollToAnchor(arg.anchor);
      }
    },
    [scrollToAnchor, onNavigateToDocument]
  );

  const internalPostProcess = useCallback((iframe: HTMLIFrameElement) => {
    postProcessIframe(iframe, {
      currentFilePath,
      projectFilePaths,
      onQmdLinkClick: handleQmdLinkClick,
    });
  }, [currentFilePath, projectFilePaths, handleQmdLinkClick]);

  // Update iframe content when HTML changes
  useEffect(() => {
    const iframe = iframeRef.current;
    if (!iframe) return;

    // Check if this is the first time we're setting content
    // An uninitialized iframe document will have an empty body
    const isFirstLoad = !isInitializedRef.current;

    if (isFirstLoad) {
      // Initial load: use `srcdoc` so the browser parses the payload as
      // a fresh HTML document with the inline `<!DOCTYPE html>` honored.
      //
      // Why not the more direct `doc.open(); doc.write(html); doc.close()`?
      // On Firefox, that pattern has historically left the document in
      // Quirks Mode even when `html` starts with `<!DOCTYPE html>`,
      // because `document.open()` on an already-initialized about:blank
      // document can retain the original compatMode rather than
      // re-deriving it from the written DOCTYPE. Chrome re-derives it
      // correctly, which is why the bug only manifests in Firefox.
      // Symptoms: external stylesheets load (the `<link>` is rewritten
      // and the data URI resolves), but Quirks Mode alters how the
      // cascade resolves external rules against UA defaults — making
      // highlight colors and `pre > code { display: block }` silently
      // fail to apply.
      //
      // `srcdoc` is parsed as a standalone HTML document, identical in
      // all browsers, so the DOCTYPE takes effect. The load is async
      // (we wait for the `load` event before post-processing), but
      // subsequent morphdom updates are still synchronous against the
      // settled contentDocument.
      const handleLoad = () => {
        isInitializedRef.current = true;
        internalPostProcess(iframe);
        // The settled document now exists; let the listener effect re-run and
        // attach scroll / click / selectionchange to it.
        setDocumentReady(true);
      };
      iframe.addEventListener('load', handleLoad, { once: true });
      iframe.srcdoc = html;

      // If the effect re-runs with a new `html` before the previous
      // load event fires, the cleanup below removes the stale listener
      // so only the most recent srcdoc's post-process runs.
      return () => {
        iframe.removeEventListener('load', handleLoad);
      };
    }

    // Subsequent updates: morphdom against the already-initialized doc.
    if (!iframe.contentDocument || !iframe.contentWindow) return;
    const doc = iframe.contentDocument;
    const win = iframe.contentWindow;

    // Save scroll position before morphing
    const scrollPos = {
      x: win.scrollX,
      y: win.scrollY,
    };

    // Create a temporary container with the new HTML
    const tempContainer = doc.createElement('html');
    tempContainer.innerHTML = html;

    // Morph the document's documentElement — updates both <head> and
    // <body> efficiently in place.
    morphdom(doc.documentElement, tempContainer);

    // Post-process after morphing
    internalPostProcess(iframe);

    // Restore scroll position. requestAnimationFrame ensures the DOM
    // has been updated before we scroll.
    requestAnimationFrame(() => {
      win.scrollTo(scrollPos.x, scrollPos.y);
    });
  }, [html, internalPostProcess]);

  // Expose methods via ref
  useImperativeHandle(ref, () => ({
    scrollToLine: (line: number) => scrollIframeToLine(iframeRef.current, line),
    getScrollRatio: () => getIframeScrollRatio(iframeRef.current),
    setScrollRatio: (ratio: number) => {
      const iframe = iframeRef.current;
      if (!iframe?.contentWindow || !iframe?.contentDocument) return;
      const maxScroll = iframe.contentDocument.documentElement.scrollHeight - iframe.contentWindow.innerHeight;
      if (maxScroll > 0) {
        iframe.contentWindow.scrollTo({ top: ratio * maxScroll });
      }
    },
    setSelection: (startPos: SourceLocation, endPos: SourceLocation) => {
      const iframe = iframeRef.current;
      const doc = iframe?.contentDocument;
      if (!doc) return;

      // Find the most specific (smallest range) elements for start and end positions
      // Now considering both line AND column for position matching
      const elements = doc.querySelectorAll('span[data-loc]');
      let startElement: HTMLElement | null = null;
      let startLoc: SourceLocation | null = null;
      let startRangeSize = Infinity;
      let endElement: HTMLElement | null = null;
      let endLoc: SourceLocation | null = null;
      let endRangeSize = Infinity;

      for (const element of elements) {
        const dataLoc = element.getAttribute('data-loc');
        if (!dataLoc) continue;
        if (element.firstChild?.nodeType !== Node.TEXT_NODE) continue;

        const loc = parseDataLoc(dataLoc);
        if (loc === null) continue;

        // Check if this element contains the start position (considering both line and column)
        if (isPositionAfterOrAt(startPos.startLine, startPos.startCol, loc.startLine, loc.startCol) &&
          isPositionBeforeOrAt(startPos.startLine, startPos.startCol, loc.endLine, loc.endCol)) {
          const rangeSize = loc.endLine - loc.startLine;
          // Prefer smaller (more specific) ranges
          if (rangeSize < startRangeSize) {
            startElement = element as HTMLElement;
            startLoc = loc;
            startRangeSize = rangeSize;
          }
        }

        // Check if this element contains the end position (considering both line and column)
        if (isPositionAfterOrAt(endPos.endLine, endPos.endCol, loc.startLine, loc.startCol) &&
          isPositionBeforeOrAt(endPos.endLine, endPos.endCol, loc.endLine, loc.endCol)) {
          const rangeSize = loc.endLine - loc.startLine;
          // Prefer smaller (more specific) ranges
          if (rangeSize < endRangeSize) {
            endElement = element as HTMLElement;
            endLoc = loc;
            endRangeSize = rangeSize;
          }
        }
      }

      // If we couldn't find matching elements, return
      if (!startElement || !endElement || !startLoc || !endLoc) {
        return;
      }

      // Calculate the approximate text offsets within the elements
      const startInfo = {
        textNode: startElement.firstChild!,
        offset: startPos.startCol - startLoc.startCol
      }
      const endInfo = {
        textNode: endElement.firstChild!,
        offset: endPos.startCol - endLoc.startCol
      }

      // Create a range and set it as the document selection
      const selection = doc.getSelection();
      if (!selection) return;

      try {
        const range = doc.createRange();
        range.setStart(startInfo.textNode, startInfo.offset);
        range.setEnd(endInfo.textNode, endInfo.offset);

        selection.removeAllRanges();
        selection.addRange(range);
      } catch (e) {
        console.error('could not set selection', startInfo, endInfo)
        return
      }
    },
    clearSelection: () => {
      const iframe = iframeRef.current;
      const doc = iframe?.contentDocument;
      if (!doc) return;

      const selection = doc.getSelection();
      if (!selection) return;

      selection.removeAllRanges();
    },
  }), []);

  // Set up event listeners on iframe. Gated on `documentReady` so the
  // listeners bind to the post-load document (the srcdoc load replaces it
  // asynchronously), re-running if that document or any callback changes.
  useEffect(() => {
    if (!documentReady) return;
    const iframe = iframeRef.current;
    if (!iframe?.contentWindow || !iframe?.contentDocument) return;

    const handleScroll = () => {
      onScroll?.();
    };

    const handleClick = () => {
      onClick?.();
    };

    const handleSelectionChange = () => {
      if (!onSelectionChange) return;

      const doc = iframe.contentDocument;
      if (!doc) return;

      const selection = doc.getSelection();
      if (!selection || selection.rangeCount === 0) return;

      // Get anchor and focus nodes with their offsets
      const anchorNode = selection.anchorNode;
      const focusNode = selection.focusNode;
      const anchorOffset = selection.anchorOffset;
      const focusOffset = selection.focusOffset;

      if (anchorNode?.nodeType === Node.TEXT_NODE && focusNode?.nodeType === Node.TEXT_NODE) {
        if (anchorNode.parentElement?.tagName !== 'SPAN' || focusNode.parentElement?.tagName !== 'SPAN') return;
        const anchorSpan = anchorNode.parentElement;
        const focusSpan = focusNode.parentElement;
        const anchorLoc = parseDataLoc(anchorSpan.getAttribute('data-loc')!);
        const focusLoc = parseDataLoc(focusSpan.getAttribute('data-loc')!);
        if (anchorLoc === null || focusLoc === null) return;

        // Guard: skip this call entirely — no setSelection/focus/reveal at
        // all — when either end resolves outside the current file (fileId
        // !== 0, e.g. inside `{{< include >}}`'d content). Fix, not
        // pre-existing behavior (2026-08-22 click-align-editor-y plan,
        // decision A8): before this, a selection inside included content
        // moved the editor's caret + focus to a same-numbered but unrelated
        // line of the currently-open file. Reuses `lineForClickTarget`
        // (already relied on below for `hostY`) rather than re-deriving its
        // checks — on this path only the fileId check is actually
        // reachable (`#q2-active-edit-region` never appears outside
        // q2-preview, and sections never carry a real `data-loc` here
        // either), but reusing the function catches all three should that
        // ever change.
        if (lineForClickTarget(anchorSpan) === null || lineForClickTarget(focusSpan) === null) {
          return;
        }

        const start = addOffsetToPosition(qmdContent, anchorLoc.startLine, anchorLoc.startCol, anchorOffset)
        const end = addOffsetToPosition(qmdContent, focusLoc.startLine, focusLoc.startCol, focusOffset)
        if (start === null || end === null) return;

        // hostY: align the anchor SPAN's on-screen y to the same line in
        // the editor (2026-08-22 click-align-editor-y plan, Phase 2).
        // Anchored on the SPAN itself, not a containing block: the
        // SourceLocation below is already span/column precision (via
        // addOffsetToPosition), so a coarser block's top would desync from
        // the very line being reported on any multi-line block. Mirrors
        // Q2PreviewIframe's `blockTop + iframeTop` pattern — the span's
        // rect is in the IFRAME's viewport, so the iframe element's own top
        // in the host page has to be added.
        const hostY =
          anchorSpan.getBoundingClientRect().top + iframe.getBoundingClientRect().top;

        onSelectionChange({
          startCol: start.col,
          startLine: start.row,
          endCol: 0, // 0s and fileId don't need to be set, but I don't want to upset typescript
          endLine: 0,
          fileId: anchorLoc.fileId
        }, {
          startCol: 0, // 0s and fileId don't need to be set, but I don't want to upset typescript
          startLine: 0,
          endCol: end.col,
          endLine: end.row,
          fileId: anchorLoc.fileId
        }, hostY);
      }
    };

    // Listen to scroll on the iframe's content window
    iframe.contentWindow.addEventListener('scroll', handleScroll, { passive: true });
    // Listen to click on the iframe's document
    iframe.contentDocument.addEventListener('click', handleClick);
    // Listen to selectionchange on the iframe's document
    iframe.contentDocument.addEventListener('selectionchange', handleSelectionChange);

    return () => {
      iframe.contentWindow?.removeEventListener('scroll', handleScroll);
      iframe.contentDocument?.removeEventListener('click', handleClick);
      iframe.contentDocument?.removeEventListener('selectionchange', handleSelectionChange);
    };
  }, [documentReady, onScroll, onClick, onSelectionChange]);

  return (
    <iframe
      ref={iframeRef}
      title="Preview"
      sandbox={'allow-same-origin allow-popups'}
      className="preview-active"
    />
  );
}

export default MorphIframe;
