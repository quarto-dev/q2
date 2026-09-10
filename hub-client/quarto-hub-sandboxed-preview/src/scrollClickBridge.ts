/**
 * Iframe-side half of editor↔preview scroll sync and click-to-line.
 *
 * In q2-preview the parent reads the same-origin iframe's
 * `contentDocument`/`contentWindow` directly (`Q2PreviewIframe.tsx` +
 * `scrollSyncDom.ts`). Cross-origin, those reads are impossible — so the
 * same pure DOM helpers run *inside* this frame, and only the results
 * travel over postMessage:
 *
 *   parent ──SCROLL_TO_LINE {line}──▶ iframe   (editor→preview)
 *   iframe ──PREVIEW_SCROLLED {ratio}──▶ parent (preview→editor scroll)
 *   iframe ──CLICK_AT_LINE {line, iframeY}──▶ parent (click-to-editor;
 *     the parent adds its own iframe.getBoundingClientRect().top to
 *     produce the host-page `hostY` the editor aligns to)
 *
 * `lineForClickTarget` / `findElementForLine` / `isElementVisible` are
 * imported unmodified from `@quarto/preview-renderer` — the measurement
 * logic is identical to q2-preview's; only where it runs differs.
 */
import {
    findElementForLine,
    isElementVisible,
    lineForClickTarget,
} from '@quarto/preview-renderer/iframe/scrollSyncDom';

/**
 * Current scroll ratio of this document: 0 at the top, 1 at the bottom,
 * 0 for a document too short to scroll. (In-frame equivalent of
 * `getIframeScrollRatio` — no null case, the document always exists here.)
 */
export function currentScrollRatio(win: Window, doc: Document): number {
    const maxScroll = doc.documentElement.scrollHeight - win.innerHeight;
    if (maxScroll <= 0) return 0;
    return win.scrollY / maxScroll;
}

/**
 * Editor→preview: bring the element mapped to `line` into view, centered,
 * unless it is already fully visible. (In-frame equivalent of
 * `scrollIframeToLine`.)
 */
export function scrollDocumentToLine(doc: Document, win: Window, line: number): void {
    const element = findElementForLine(doc, line);
    if (!element) return;
    if (!isElementVisible(element, win)) {
        element.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
}

/**
 * Attach the preview→editor listeners: window scroll → PREVIEW_SCROLLED,
 * capture-phase pointerup → CLICK_AT_LINE. Returns an uninstaller.
 *
 * Capture-phase pointerup for the same reason as Q2PreviewIframe: the
 * block-edit activation (bubble-phase pointerup) replaces the clicked
 * subtree, after which no `click` is dispatched at all — the capture
 * handler runs first, while the target's `[data-loc]` ancestor is still
 * attached.
 */
export function installScrollClickBridge(
    win: Window,
    doc: Document,
    post: (msg: unknown) => void,
): () => void {
    const handleScroll = () => {
        post({ type: 'PREVIEW_SCROLLED', ratio: currentScrollRatio(win, doc) });
    };
    const handlePointerUp = (e: Event) => {
        const line = lineForClickTarget(e.target);
        if (line === null) return;
        // Report the clicked block's top edge in THIS frame's viewport
        // coordinates; the parent converts to host-page coordinates.
        const target = e.target as { closest?: Element['closest'] } | null;
        const block = target?.closest?.('[data-loc]');
        const iframeY = block?.getBoundingClientRect().top;
        post({ type: 'CLICK_AT_LINE', line, iframeY });
    };

    win.addEventListener('scroll', handleScroll, { passive: true });
    doc.addEventListener('pointerup', handlePointerUp, true);
    return () => {
        win.removeEventListener('scroll', handleScroll);
        doc.removeEventListener('pointerup', handlePointerUp, true);
    };
}
