// dragSelectionCapture.ts (bd-abo9m23f) — classify the DOM selection at a
// mouse activation into the editor's opening-selection payload.
//
// bd-q9lyghv2 bridged a mouse CLICK across the render→editor DOM swap by
// stashing the click's viewport coordinates and replaying them through
// posAtCoords at editor mount. This module extends that bridge to a mouse
// DRAG: when the user releases a drag whose selection is fully contained in
// the block being activated, we capture BOTH selection endpoints (viewport
// coords, direction-aware) so the editor can open with the equivalent
// selection instead of a bare caret — making the selection-driven toolbar
// (bold, link, …) immediately usable.
//
// A selection that reaches OUTSIDE the activated block can't be represented
// in the single-block editor. Worse, activating would swap the DOM under the
// user's feet and destroy a selection they may have made just to copy text —
// so cross-block drags suppress the activation entirely.
//
// Same geometric premise as caret-at-click: the editor renders the same
// visual text in the same measured box (same theme CSS), so endpoint
// coordinates captured before the swap land on the same glyphs after it.
// jsdom can't do this geometry (getClientRects is empty) — unit tests mock
// the rects; truth lives in the browser e2e test.

/** A point in viewport (client) coordinate space. */
export interface ViewportPoint {
    x: number;
    y: number;
}

/**
 * The opening-selection payload stashed on `pendingOpenSelectionRef` by the
 * activation site and consumed (read-once) by `RichTextEditor` at mount.
 *
 * - `caret`: place the caret at `head` (the click / release point) — the
 *   bd-q9lyghv2 behavior.
 * - `range`: recreate a drag selection from `anchor` to `head`. Direction is
 *   preserved (`head` is the selection's focus — where the drag released) so
 *   Shift-Arrow keeps extending from the right end.
 */
export type PendingOpenSelection =
    | { kind: 'caret'; head: ViewportPoint }
    | { kind: 'range'; anchor: ViewportPoint; head: ViewportPoint };

/**
 * Classify the live DOM selection for a mouse activation of `outerBlock`.
 *
 * @returns
 * - `{ kind: 'range', anchor, head }` — non-collapsed selection fully inside
 *   `outerBlock`, endpoints readable → open with the equivalent selection.
 * - `'suppress'` — non-collapsed selection NOT contained in `outerBlock`
 *   (cross-block drag) → caller must abort the activation, leaving the
 *   user's selection intact.
 * - `{ kind: 'caret', head: clickCoords }` — everything else (collapsed
 *   selection i.e. plain click, no ranges, unreadable endpoint geometry) →
 *   today's caret-at-click behavior.
 *
 * Mouse-only by construction: keyboard/touch activations carry no click
 * coords and never reach this classification.
 */
export function classifyOpenSelection(
    outerBlock: Element,
    clickCoords: ViewportPoint,
): PendingOpenSelection | 'suppress' {
    const caret: PendingOpenSelection = { kind: 'caret', head: clickCoords };
    const doc = outerBlock.ownerDocument;
    // ownerDocument-relative, not the bare global: the preview runs inside an
    // iframe and must read that iframe's selection.
    const sel = doc.defaultView?.getSelection();
    if (!sel || sel.isCollapsed || sel.rangeCount === 0) return caret;

    const range = sel.getRangeAt(0);
    const contained =
        outerBlock.contains(range.startContainer) &&
        outerBlock.contains(range.endContainer);
    if (!contained) return 'suppress';

    // Endpoint coords come from the SELECTION's anchor/focus (not the
    // normalized range start/end) so a backward drag keeps its direction.
    const anchor = endpointViewportPoint(doc, sel.anchorNode, sel.anchorOffset);
    const head = endpointViewportPoint(doc, sel.focusNode, sel.focusOffset);
    if (!anchor || !head) return caret;

    return { kind: 'range', anchor, head };
}

/**
 * Viewport coordinates of a selection endpoint, via a collapsed range's
 * client rect. Vertically centered in the endpoint's line box so the replayed
 * posAtCoords lands inside the line, not on its boundary.
 *
 * Returns null when the geometry is unreadable (invalid offset, or no rect —
 * e.g. an endpoint between element boundaries); the caller falls back to the
 * caret payload.
 */
function endpointViewportPoint(
    doc: Document,
    node: Node | null,
    offset: number,
): ViewportPoint | null {
    if (!node) return null;
    const r = doc.createRange();
    try {
        r.setStart(node, offset);
    } catch {
        return null;
    }
    r.collapse(true);
    const rect = r.getClientRects()[0] ?? r.getBoundingClientRect();
    if (!rect || (rect.width === 0 && rect.height === 0 && rect.left === 0 && rect.top === 0)) {
        return null;
    }
    return { x: rect.left, y: rect.top + rect.height / 2 };
}
