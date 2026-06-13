/**
 * caretGeometry.ts — caret-geometry utilities for cross-surface cursor (Plan P2.4a).
 *
 * Two distinct concerns are kept cleanly separated:
 *
 * 1. **Visual-line detection** (geometry) — answers "is the caret on the first or
 *    last *visual* (soft-wrapped) row of a textarea?" This requires real browser
 *    layout via a mirror div. These functions are intentionally exported as
 *    standalone named exports so P2.4b can mock them with vi.spyOn/vi.mock.
 *
 *    ⚠ jsdom limitation: `offsetTop` and `scrollHeight` are always 0 in jsdom,
 *    so the geometric *correctness* of these functions is verified by Playwright
 *    in P2.5 (real browser). jsdom tests only verify structural behavior
 *    (mirror setup, cleanup, boolean return).
 *
 * 2. **Logical column** (pure string math) — captures and places the caret column
 *    within the current `\n`-delimited line of the textarea's `value`. Fully
 *    unit-testable in jsdom.
 */

// ── Helpers ───────────────────────────────────────────────────────────────────

/**
 * Derive a numeric line-height in pixels from computed styles.
 *
 * `getComputedStyle` returns 'normal' for `line-height` when the author hasn't
 * set it explicitly. In that case we fall back to 1.2 × the computed font-size
 * (a common browser default for proportional fonts). If font-size is also
 * unparseable we fall back to 16px (the browser default for body text).
 */
function resolveLineHeightPx(cs: CSSStyleDeclaration): number {
    const lh = cs.lineHeight;
    if (lh && lh !== 'normal') {
        const n = parseFloat(lh);
        if (Number.isFinite(n) && n > 0) return n;
    }
    // Fallback: 1.2 × font-size
    const fs = parseFloat(cs.fontSize);
    return Number.isFinite(fs) && fs > 0 ? fs * 1.2 : 16 * 1.2;
}

/**
 * Build a mirror div styled identically to `ta` so that text wraps at the
 * same column boundaries. The caller is responsible for appending and removing
 * the div; this function only creates and styles it.
 */
function buildMirrorDiv(ta: HTMLTextAreaElement): HTMLDivElement {
    const cs = getComputedStyle(ta);
    const mirror = document.createElement('div');

    // Typography — must match exactly for wrap fidelity.
    mirror.style.font = cs.font;                          // shorthand: family + size + weight + style
    mirror.style.fontFamily = cs.fontFamily;              // belt-and-suspenders for browsers that split `font`
    mirror.style.fontSize = cs.fontSize;
    mirror.style.fontWeight = cs.fontWeight;
    mirror.style.fontStyle = cs.fontStyle;
    mirror.style.letterSpacing = cs.letterSpacing;
    mirror.style.textTransform = cs.textTransform;
    mirror.style.tabSize = cs.tabSize;

    // Spacing — must match exactly so the line-height and padding offsets are
    // numerically the same as they would be inside the textarea.
    mirror.style.lineHeight = cs.lineHeight;
    mirror.style.padding = cs.padding;
    mirror.style.paddingTop = cs.paddingTop;
    mirror.style.paddingRight = cs.paddingRight;
    mirror.style.paddingBottom = cs.paddingBottom;
    mirror.style.paddingLeft = cs.paddingLeft;
    mirror.style.borderWidth = '0';                       // border is irrelevant to text layout
    mirror.style.boxSizing = cs.boxSizing;

    // Width: use clientWidth (content area + padding, no border, no scrollbar)
    // so the mirror reflects the exact wrapping width the textarea uses. Using
    // offsetWidth would include borders; using a declared CSS width could differ
    // from the textarea's actual rendered width if a scrollbar is present.
    mirror.style.width = `${ta.clientWidth}px`;

    // Whitespace/wrap behaviour must match a textarea.
    mirror.style.whiteSpace = 'pre-wrap';
    mirror.style.wordWrap = 'break-word';         // IE compat alias
    mirror.style.overflowWrap = 'break-word';

    // Position off-screen so layout is computed but nothing is visible.
    mirror.style.position = 'absolute';
    mirror.style.visibility = 'hidden';
    mirror.style.left = '-9999px';
    mirror.style.top = '0';

    return mirror;
}

// ── Visual-line detection ─────────────────────────────────────────────────────

/**
 * Returns `true` if the textarea's caret is on the **first visual (soft-wrapped)
 * row**. Uses a mirror div to measure geometry; cleans up even on exception.
 *
 * **Threshold:** the marker's `offsetTop` must be less than one line-height from
 * the top of the mirror (i.e., `offsetTop < lineHeightPx`). A top-padding offset
 * is NOT subtracted because both the mirror and the marker measure from the same
 * origin, so padding cancels out.
 *
 * **jsdom note:** In jsdom, `offsetTop` / `scrollHeight` are always 0, so this
 * function always returns `true` there. Geometric correctness is verified by
 * Playwright (P2.5).
 */
export function isOnFirstVisualLine(ta: HTMLTextAreaElement): boolean {
    const cs = getComputedStyle(ta);
    const lineHeightPx = resolveLineHeightPx(cs);
    const mirror = buildMirrorDiv(ta);
    document.body.appendChild(mirror);
    try {
        // Fill the mirror with everything BEFORE the caret, then place a
        // zero-size marker span. Its offsetTop tells us which visual row the
        // caret occupies.
        const beforeCaret = ta.value.slice(0, ta.selectionStart ?? 0);
        mirror.textContent = beforeCaret;
        const marker = document.createElement('span');
        mirror.appendChild(marker);

        return marker.offsetTop < lineHeightPx;
    } finally {
        document.body.removeChild(mirror);
    }
}

/**
 * Returns `true` if the textarea's caret is on the **last visual (soft-wrapped)
 * row**. Uses a mirror div to measure geometry; cleans up even on exception.
 *
 * **Threshold:** we measure the marker's `offsetTop` (caret row) and compare it
 * to the total content height of the same mirror when it contains the *full*
 * value. The caret's visual row is the last row iff the row below it would
 * start at or past the full content height:
 *
 *   `markerOffsetTop + lineHeightPx + LAST_LINE_TOLERANCE >= fullHeight`
 *
 * We measure `fullHeight` as the mirror's `scrollHeight` after filling it with
 * the entire `ta.value`.
 *
 * **Sub-pixel tolerance:** In real browsers (Chromium), `scrollHeight` is an
 * integer while `lineHeightPx` is a fractional CSS value (e.g. 22.95px). For a
 * single-line textarea, `markerOffsetTop` is 0 and `scrollHeight` is always
 * rounded up (e.g. 23px), so `0 + 22.95 < 23` — a false negative. We add a
 * 2px tolerance to absorb this sub-pixel rounding without introducing
 * false positives on multi-line content where rows differ by a full line-height.
 *
 * **jsdom note:** In jsdom, `offsetTop` / `scrollHeight` are always 0, so this
 * function always returns `true` there. Geometric correctness is verified by
 * Playwright (P2.5).
 */

/** Tolerance (px) added to the last-visual-line check to absorb sub-pixel
 *  `scrollHeight` rounding in real browsers (Chromium rounds up; Firefox may
 *  too). Value chosen to be < half a typical line-height (~10px) so it cannot
 *  cause a false positive on a 2-row textarea. */
const LAST_LINE_TOLERANCE = 2;

export function isOnLastVisualLine(ta: HTMLTextAreaElement): boolean {
    const cs = getComputedStyle(ta);
    const lineHeightPx = resolveLineHeightPx(cs);
    const mirror = buildMirrorDiv(ta);
    document.body.appendChild(mirror);
    try {
        // Step 1: measure the marker offset (caret row).
        const beforeCaret = ta.value.slice(0, ta.selectionStart ?? 0);
        mirror.textContent = beforeCaret;
        const marker = document.createElement('span');
        mirror.appendChild(marker);
        const markerOffsetTop = marker.offsetTop;

        // Step 2: measure full content height (entire value).
        mirror.textContent = ta.value;
        const fullHeight = mirror.scrollHeight;

        // The caret is on the last row if there is less than one line-height
        // of content below the marker's row start. We add LAST_LINE_TOLERANCE
        // to absorb sub-pixel scrollHeight rounding in real browsers.
        return markerOffsetTop + lineHeightPx + LAST_LINE_TOLERANCE >= fullHeight;
    } finally {
        document.body.removeChild(mirror);
    }
}

// ── Logical column (pure string math) ────────────────────────────────────────

/**
 * Returns the caret's column (0-based character offset) within the current
 * *logical* (`\n`-delimited) line.
 *
 * Example: value = "hello\nworld", selectionStart = 8 ("orl" not yet reached)
 *   → the caret is 2 chars into "world" → column 2.
 *
 * If `selectionStart` is null (should not happen in modern browsers but
 * TypeScript allows it), returns 0.
 */
export function getLogicalColumn(ta: HTMLTextAreaElement): number {
    const pos = ta.selectionStart ?? 0;
    const text = ta.value;
    // Find the start of the logical line that contains `pos`.
    const lineStart = text.lastIndexOf('\n', pos - 1) + 1; // +1 skips the '\n'; 0 if none
    return pos - lineStart;
}

/**
 * Places the caret on the first (`edge='first'`) or last (`edge='last'`)
 * *logical* (`\n`-delimited) line of `ta.value`, at `min(column, lineLength)`.
 *
 * This is used when navigating INTO a tile: arrival via ↓ lands on the first
 * logical line; arrival via ↑ lands on the last.
 *
 * **Known limitation (documented, not fixed here):** `setSelectionRange` resets
 * the browser's native goal column, so a desired column is not preserved past a
 * short arrival line on subsequent arrow-key presses. Carrying the goal column
 * across tiles is a refinement deferred to a later phase; v1 capture-and-clamp
 * is sufficient for the cross-surface jump.
 *
 * @param ta     The textarea to update.
 * @param edge   'first' → land on the first logical line; 'last' → last line.
 * @param column Desired column (0-based). Clamped to the line length.
 */
export function placeCaretAtColumn(
    ta: HTMLTextAreaElement,
    edge: 'first' | 'last',
    column: number,
): void {
    const lines = ta.value.split('\n');
    // `''.split('\n')` yields [''], so lines.length is always ≥ 1 here.

    if (edge === 'first') {
        const line = lines[0];
        const clampedCol = Math.min(column, line.length);
        ta.selectionStart = ta.selectionEnd = clampedCol;
    } else {
        // edge === 'last'
        // Compute the character offset of the start of the last logical line.
        const lastLineIndex = lines.length - 1;
        let lineStartOffset = 0;
        for (let i = 0; i < lastLineIndex; i++) {
            lineStartOffset += lines[i].length + 1; // +1 for '\n'
        }
        const lastLine = lines[lastLineIndex];
        const clampedCol = Math.min(column, lastLine.length);
        ta.selectionStart = ta.selectionEnd = lineStartOffset + clampedCol;
    }
}
