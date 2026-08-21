/**
 * Shared DOM helpers for editor↔preview scroll sync. Both the HTML
 * preview (`MorphIframe`) and the q2-preview iframe (`Q2PreviewIframe`)
 * map an editor line number onto a preview element via `data-loc`
 * attributes of the form `fileId:startLine:startCol-endLine:endCol`
 * (1-based), the same format the native HTML writer and q2-preview's
 * `dataLocProps` emit.
 */

/**
 * Parsed source location from a `data-loc` attribute.
 * Format: `fileId:startLine:startCol-endLine:endCol` (1-based).
 */
export interface SourceLocation {
    fileId: number;
    startLine: number;
    startCol: number;
    endLine: number;
    endCol: number;
}

/**
 * Parse a `data-loc` attribute string into a `SourceLocation`.
 * Returns null if the format is invalid.
 */
export function parseDataLoc(dataLoc: string): SourceLocation | null {
    const match = dataLoc.match(/^(\d+):(\d+):(\d+)-(\d+):(\d+)$/);
    if (!match) return null;
    return {
        fileId: parseInt(match[1], 10),
        startLine: parseInt(match[2], 10),
        startCol: parseInt(match[3], 10),
        endLine: parseInt(match[4], 10),
        endCol: parseInt(match[5], 10),
    };
}

/**
 * Find the best matching element for a given line number, preferring
 * the most specific (smallest line range) match that contains the line.
 *
 * When no element's range contains the line (a blank line between
 * blocks, or — most commonly — a fresh line past the last block at the
 * end of the document), falls back to the nearest located block: the
 * one starting closest at-or-before the line, or failing that the first
 * block after it. This keeps end-of-document edits scrolling the
 * preview instead of silently no-op'ing. Returns null only when the
 * document has no located elements at all.
 */
export function findElementForLine(
    doc: Document,
    line: number,
): HTMLElement | null {
    const elements = doc.querySelectorAll('[data-loc]');
    let bestMatch: HTMLElement | null = null;
    let bestRangeSize = Infinity;

    // Fallbacks for a line no range contains: nearest block at-or-before
    // (largest startLine ≤ line) preferred, else nearest block after
    // (smallest startLine > line).
    let precedingMatch: HTMLElement | null = null;
    let precedingStart = -Infinity;
    let followingMatch: HTMLElement | null = null;
    let followingStart = Infinity;

    for (const element of elements) {
        const dataLoc = element.getAttribute('data-loc');
        if (!dataLoc) continue;

        const loc = parseDataLoc(dataLoc);
        if (!loc) continue;

        if (line >= loc.startLine && line <= loc.endLine) {
            const rangeSize = loc.endLine - loc.startLine;
            if (rangeSize < bestRangeSize) {
                bestMatch = element as HTMLElement;
                bestRangeSize = rangeSize;
            }
        } else if (loc.startLine <= line && loc.startLine > precedingStart) {
            precedingMatch = element as HTMLElement;
            precedingStart = loc.startLine;
        } else if (loc.startLine > line && loc.startLine < followingStart) {
            followingMatch = element as HTMLElement;
            followingStart = loc.startLine;
        }
    }

    return bestMatch ?? precedingMatch ?? followingMatch;
}

/**
 * Preview→editor click sync (click-to-editor-scroll, bd-9kzfi follow-up):
 * resolve a `pointerup` event target to the source line it should reveal in
 * the editor, or `null` when the click should be inert.
 *
 * Walks up from `target` via `closest`, so the innermost located ancestor
 * wins (never a document-wide `querySelector`, which would ignore the
 * target entirely). Three cases return `null` rather than a line:
 *
 *  - the target is inside `#q2-active-edit-region` — q2-preview's block
 *    activation replaces the clicked subtree with this synthetic region on
 *    the same `pointerup`; a caret move inside an already-open editor must
 *    not yank the editor to the enclosing block.
 *  - the nearest `[data-loc]` ancestor is a `<section>` — sections carry a
 *    `data-loc` too (`SectionizeTransform` is unconditional), so
 *    inter-block whitespace or an edit region nested inside a section would
 *    otherwise resolve to the section's own (usually distant) line.
 *  - there is no `[data-loc]` ancestor at all.
 *
 * Narrows with a duck-typed `.closest` check rather than `instanceof
 * Element`: in production this runs in the *parent* frame's realm against a
 * `pointerup` target from the sandboxed *iframe*'s realm, and each realm has
 * its own `Element` constructor — `target instanceof Element` (the parent's
 * `Element`) is always false for an iframe-realm node, even a real `<p>`.
 * `closest` is realm-agnostic duck typing that still excludes `Document`,
 * `Window`, and other non-Element `EventTarget`s (none of which have it).
 */
export function lineForClickTarget(target: EventTarget | null): number | null {
    if (target === null || typeof (target as { closest?: unknown }).closest !== 'function') {
        return null;
    }
    const el = target as Element;
    if (el.closest('#q2-active-edit-region')) return null;

    const locEl = el.closest('[data-loc]');
    if (!locEl) return null;
    if (locEl.tagName.toLowerCase() === 'section') return null;

    const dataLoc = locEl.getAttribute('data-loc');
    if (!dataLoc) return null;
    const loc = parseDataLoc(dataLoc);
    if (!loc) return null;

    return loc.startLine;
}

/**
 * Check if an element is fully visible within the iframe viewport.
 * `win` is the iframe's own `contentWindow` (so `innerHeight` is the
 * preview viewport, not the host page's).
 */
export function isElementVisible(element: HTMLElement, win: Window): boolean {
    const rect = element.getBoundingClientRect();
    return rect.top >= 0 && rect.bottom <= win.innerHeight;
}

/**
 * Editor→preview scroll: bring the element mapped to `line` into view,
 * centered, but only if it isn't already fully visible. No-op when the iframe
 * isn't ready or no element maps to the line. The single scroll-to-line code
 * path shared by MorphIframe and Q2PreviewIframe.
 */
export function scrollIframeToLine(
    iframe: HTMLIFrameElement | null,
    line: number,
): void {
    const doc = iframe?.contentDocument;
    const win = iframe?.contentWindow;
    if (!doc || !win) return;
    const element = findElementForLine(doc, line);
    if (!element) return;
    if (!isElementVisible(element, win)) {
        element.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
}

/**
 * Current scroll ratio of the iframe document: 0 at the top, 1 at the bottom,
 * 0 for a document too short to scroll, and null when the iframe isn't ready.
 * Shared by MorphIframe and Q2PreviewIframe.
 */
export function getIframeScrollRatio(iframe: HTMLIFrameElement | null): number | null {
    const win = iframe?.contentWindow;
    const doc = iframe?.contentDocument;
    if (!win || !doc) return null;
    const maxScroll = doc.documentElement.scrollHeight - win.innerHeight;
    if (maxScroll <= 0) return 0;
    return win.scrollY / maxScroll;
}
