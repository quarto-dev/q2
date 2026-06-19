// @vitest-environment jsdom
/**
 * Tests for caretGeometry.ts (Plan P2.4a).
 *
 * Two categories:
 *
 * A. **Logical-column tests** (pure string math): fully testable in jsdom.
 *    These tests are written TDD-first — they fail before implementation and
 *    pass after.
 *
 * B. **Visual-line structural tests** (mirror-div): jsdom cannot provide real
 *    layout (`offsetTop`/`scrollHeight` are always 0), so we do NOT assert
 *    geometric values. Instead we verify:
 *      - The functions return a boolean without throwing.
 *      - The mirror div is cleaned up (no orphan nodes on document.body).
 *      - Cleanup still happens even when an exception is thrown.
 *    Geometric correctness is verified by Playwright in P2.5 (real browser).
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import {
    getLogicalColumn,
    placeCaretAtColumn,
    isOnFirstVisualLine,
    isOnLastVisualLine,
    prefixWidth,
} from './caretGeometry';
import { buildByteLineMap } from '../utils/byteLineMap';

// ── Test helpers ──────────────────────────────────────────────────────────────

/**
 * Create a minimal HTMLTextAreaElement suitable for jsdom testing.
 * We append it to document.body so that getComputedStyle works, and
 * remove it in afterEach.
 */
function makeTa(value: string, selectionStart: number): HTMLTextAreaElement {
    const ta = document.createElement('textarea');
    ta.value = value;
    ta.selectionStart = selectionStart;
    ta.selectionEnd = selectionStart;
    document.body.appendChild(ta);
    return ta;
}

const created: HTMLTextAreaElement[] = [];

function ta(value: string, sel: number): HTMLTextAreaElement {
    const el = makeTa(value, sel);
    created.push(el);
    return el;
}

afterEach(() => {
    while (created.length) {
        const el = created.pop()!;
        if (el.parentNode) el.parentNode.removeChild(el);
    }
});

// ── A. Logical-column: getLogicalColumn ───────────────────────────────────────

describe('getLogicalColumn', () => {
    it('returns 0 when caret is at the very start of the value', () => {
        expect(getLogicalColumn(ta('hello\nworld', 0))).toBe(0);
    });

    it('returns the column within the first line (middle)', () => {
        // 'hello\nworld', caret at 3 → col 3 in "hello"
        expect(getLogicalColumn(ta('hello\nworld', 3))).toBe(3);
    });

    it('returns column equal to line length when at end of first line', () => {
        // 'hello\nworld', caret at 5 → col 5 (end of "hello")
        expect(getLogicalColumn(ta('hello\nworld', 5))).toBe(5);
    });

    it('returns 0 at the start of the second logical line (right after \\n)', () => {
        // 'hello\nworld', caret at 6 (start of "world") → col 0
        expect(getLogicalColumn(ta('hello\nworld', 6))).toBe(0);
    });

    it('returns the column within the second line (middle)', () => {
        // 'hello\nworld', caret at 8 → "wo" = 2 chars into "world" → col 2
        expect(getLogicalColumn(ta('hello\nworld', 8))).toBe(2);
    });

    it('returns column at end of second logical line', () => {
        // 'hello\nworld', caret at 11 (end of "world") → col 5
        expect(getLogicalColumn(ta('hello\nworld', 11))).toBe(5);
    });

    it('handles a single-line value', () => {
        // 'abcde', caret at 2 → col 2
        expect(getLogicalColumn(ta('abcde', 2))).toBe(2);
    });

    it('returns 0 when caret is at start of a single-line value', () => {
        expect(getLogicalColumn(ta('hello', 0))).toBe(0);
    });

    it('handles empty value (selectionStart = 0)', () => {
        expect(getLogicalColumn(ta('', 0))).toBe(0);
    });

    it('handles three logical lines, caret on the third', () => {
        // 'aa\nbb\ncc': indices 0='a',1='a',2='\n',3='b',4='b',5='\n',6='c',7='c'
        // caret at 6 (start of "cc") → col 0
        expect(getLogicalColumn(ta('aa\nbb\ncc', 6))).toBe(0);
    });

    it('handles three logical lines, caret mid third line', () => {
        // 'aa\nbb\ncc': caret at 7 (1 char into "cc") → col 1
        expect(getLogicalColumn(ta('aa\nbb\ncc', 7))).toBe(1);
    });

    it('handles an empty logical line (consecutive \\n)', () => {
        // 'a\n\nb', caret at 2 (the empty line) → col 0
        expect(getLogicalColumn(ta('a\n\nb', 2))).toBe(0);
    });
});

// ── A. Logical-column: placeCaretAtColumn ─────────────────────────────────────

describe('placeCaretAtColumn', () => {
    // ── edge='first' ──────────────────────────────────────────────────────────

    it('places caret at column 0 on first line', () => {
        const el = ta('hello\nworld', 8);
        placeCaretAtColumn(el, { edge: 'first', column: 0 });
        expect(el.selectionStart).toBe(0);
        expect(el.selectionEnd).toBe(0);
    });

    it('places caret at column 3 on first line', () => {
        const el = ta('hello\nworld', 8);
        placeCaretAtColumn(el, { edge: 'first', column: 3 });
        expect(el.selectionStart).toBe(3);
        expect(el.selectionEnd).toBe(3);
    });

    it('places caret at end of first line (column == lineLength)', () => {
        const el = ta('hello\nworld', 0);
        placeCaretAtColumn(el, { edge: 'first', column: 5 });
        expect(el.selectionStart).toBe(5);
    });

    it('clamps column to first-line length when column > lineLength', () => {
        // first line "hello" has length 5; column 99 → clamped to 5
        const el = ta('hello\nworld', 0);
        placeCaretAtColumn(el, { edge: 'first', column: 99 });
        expect(el.selectionStart).toBe(5);
    });

    it('clamps column to 0 when first line is empty', () => {
        // value = '\nworld', first line is ''
        const el = ta('\nworld', 5);
        placeCaretAtColumn(el, { edge: 'first', column: 3 });
        expect(el.selectionStart).toBe(0);
    });

    // ── edge='last' ───────────────────────────────────────────────────────────

    it('places caret at column 0 on last line', () => {
        // 'hello\nworld', last line starts at offset 6
        const el = ta('hello\nworld', 0);
        placeCaretAtColumn(el, { edge: 'last', column: 0 });
        expect(el.selectionStart).toBe(6);
        expect(el.selectionEnd).toBe(6);
    });

    it('places caret at column 3 on last line', () => {
        const el = ta('hello\nworld', 0);
        placeCaretAtColumn(el, { edge: 'last', column: 3 });
        expect(el.selectionStart).toBe(9); // 6 + 3
    });

    it('places caret at end of last line', () => {
        const el = ta('hello\nworld', 0);
        placeCaretAtColumn(el, { edge: 'last', column: 5 });
        expect(el.selectionStart).toBe(11); // 6 + 5
    });

    it('clamps column to last-line length when column > lineLength', () => {
        // last line "world" has length 5; column 99 → clamped to 5 → offset 11
        const el = ta('hello\nworld', 0);
        placeCaretAtColumn(el, { edge: 'last', column: 99 });
        expect(el.selectionStart).toBe(11);
    });

    it('clamps column to 0 when last line is empty', () => {
        // 'hello\n', last line is ''
        const el = ta('hello\n', 0);
        placeCaretAtColumn(el, { edge: 'last', column: 5 });
        expect(el.selectionStart).toBe(6); // offset of the empty last line
    });

    it('handles three logical lines, places on last', () => {
        // 'aa\nbb\ncc': 'aa'(2) + '\n'(1) + 'bb'(2) + '\n'(1) = offset 6 for "cc"
        const el = ta('aa\nbb\ncc', 0);
        placeCaretAtColumn(el, { edge: 'last', column: 1 });
        expect(el.selectionStart).toBe(7); // 6 + 1 ✓
    });

    // ── single-line value ─────────────────────────────────────────────────────

    it('for a single-line value, first and last refer to the same line', () => {
        const el1 = ta('hello', 0);
        const el2 = ta('hello', 0);
        placeCaretAtColumn(el1, { edge: 'first', column: 2 });
        placeCaretAtColumn(el2, { edge: 'last', column: 2 });
        expect(el1.selectionStart).toBe(2);
        expect(el2.selectionStart).toBe(2);
    });

    it('clamps on single-line value for both edges', () => {
        const el1 = ta('hi', 0);
        const el2 = ta('hi', 0);
        placeCaretAtColumn(el1, { edge: 'first', column: 99 });
        placeCaretAtColumn(el2, { edge: 'last', column: 99 });
        expect(el1.selectionStart).toBe(2);
        expect(el2.selectionStart).toBe(2);
    });

    it('handles empty value without throwing', () => {
        const el = ta('', 0);
        expect(() => placeCaretAtColumn(el, { edge: 'first', column: 0 })).not.toThrow();
        expect(() => placeCaretAtColumn(el, { edge: 'last', column: 0 })).not.toThrow();
        expect(el.selectionStart).toBe(0);
    });

    // ── { line; column } interior-line hint (§2 caret-aware nest moves) ──────────

    it('places caret on an interior logical line at the given column', () => {
        // 'aa\nbb\ncc': line 1 ("bb") starts at offset 3 → col 1 → offset 4
        const el = ta('aa\nbb\ncc', 0);
        placeCaretAtColumn(el, { line: 1, column: 1 });
        expect(el.selectionStart).toBe(4);
        expect(el.selectionEnd).toBe(4);
    });

    it('places caret on the third logical line (line 2)', () => {
        // 'aa\nbb\ncc': line 2 ("cc") starts at offset 6 → col 2 → offset 8
        const el = ta('aa\nbb\ncc', 0);
        placeCaretAtColumn(el, { line: 2, column: 2 });
        expect(el.selectionStart).toBe(8);
    });

    it('clamps the column to the interior line length', () => {
        // line 1 ("bb") length 2; column 99 → offset 3 + 2 = 5
        const el = ta('aa\nbb\ncc', 0);
        placeCaretAtColumn(el, { line: 1, column: 99 });
        expect(el.selectionStart).toBe(5);
    });

    it('clamps the line index past the last line (no overflow)', () => {
        // line 99 → clamps to last line ("cc", offset 6); column 0 → offset 6
        const el = ta('aa\nbb\ncc', 0);
        placeCaretAtColumn(el, { line: 99, column: 0 });
        expect(el.selectionStart).toBe(6);
    });

    it('line 0 interior hint matches the first-edge hint', () => {
        const el = ta('aa\nbb\ncc', 0);
        placeCaretAtColumn(el, { line: 0, column: 1 });
        expect(el.selectionStart).toBe(1);
    });
});

// ── B. Visual-line structural tests ──────────────────────────────────────────
//
// jsdom provides no real layout: `offsetTop` and `scrollHeight` are always 0.
// We therefore ONLY assert:
//   1. The function returns a boolean without throwing.
//   2. The mirror div is cleaned up — document.body contains no extra nodes
//      after the call.
//   3. Cleanup happens even when an exception is thrown inside the function.
//
// Geometric correctness is verified by Playwright (P2.5).

describe('isOnFirstVisualLine — structural (jsdom)', () => {
    it('returns a boolean without throwing', () => {
        const el = ta('hello world', 5);
        const result = isOnFirstVisualLine(el);
        expect(typeof result).toBe('boolean');
    });

    it('leaves no extra nodes on document.body after call', () => {
        const countBefore = document.body.childNodes.length;
        const el = ta('hello world', 5);
        // ta() appends to body, so count it:
        const countAfterTa = document.body.childNodes.length;
        isOnFirstVisualLine(el);
        expect(document.body.childNodes.length).toBe(countAfterTa);
    });

    it('cleans up the mirror div even if marker.offsetTop throws', () => {
        // Force a throw inside the try block by making the `offsetTop` getter
        // on HTMLElement blow up for exactly one call (the marker's read).
        const spy = vi.spyOn(HTMLElement.prototype, 'offsetTop', 'get')
            .mockImplementationOnce(() => { throw new Error('boom'); });
        try {
            const el = ta('line one\nline two', 3);
            const before = document.body.childNodes.length;
            // The function must propagate the error …
            expect(() => isOnFirstVisualLine(el)).toThrow('boom');
            // … but the finally block must have removed the mirror.
            expect(document.body.childNodes.length).toBe(before);
        } finally {
            spy.mockRestore();
        }
    });

    it('works on a multi-line value', () => {
        const el = ta('alpha\nbeta\ngamma', 0);
        expect(() => isOnFirstVisualLine(el)).not.toThrow();
    });

    it('works on an empty value', () => {
        const el = ta('', 0);
        expect(() => isOnFirstVisualLine(el)).not.toThrow();
    });
});

describe('isOnLastVisualLine — structural (jsdom)', () => {
    it('returns a boolean without throwing', () => {
        const el = ta('hello world', 5);
        const result = isOnLastVisualLine(el);
        expect(typeof result).toBe('boolean');
    });

    it('leaves no extra nodes on document.body after call', () => {
        const el = ta('hello world', 11);
        const countAfterTa = document.body.childNodes.length;
        isOnLastVisualLine(el);
        expect(document.body.childNodes.length).toBe(countAfterTa);
    });

    it('cleans up mirror div on repeated calls (no accumulation)', () => {
        const el = ta('alpha\nbeta', 6);
        const before = document.body.childNodes.length;
        isOnLastVisualLine(el);
        isOnLastVisualLine(el);
        expect(document.body.childNodes.length).toBe(before);
    });

    it('works on a multi-line value', () => {
        const el = ta('alpha\nbeta\ngamma', 14);
        expect(() => isOnLastVisualLine(el)).not.toThrow();
    });

    it('works on an empty value', () => {
        const el = ta('', 0);
        expect(() => isOnLastVisualLine(el)).not.toThrow();
    });
});

// ── B2. LAST_LINE_TOLERANCE regression (P2.5b fix A) ─────────────────────────
//
// In Chromium, scrollHeight is an integer (e.g. 27) while lineHeightPx is
// fractional (e.g. 22.95px). For a single-line textarea with markerOffsetTop=4:
//   WITHOUT tolerance: 4 + 22.95 = 26.95 < 27  → false (bug: misses the last row)
//   WITH tolerance=2:  4 + 22.95 + 2 = 28.95 >= 27 → true  (fix)
//
// We mock the geometry getters so this test does not need a real browser.
// The mock strategy:
//   - HTMLElement.prototype.offsetTop (getter): return 4 for the first read
//     (marker.offsetTop) and 0 for all subsequent reads.
//   - Element.prototype.scrollHeight (getter): return 27 for the first read
//     (mirror.scrollHeight after full content fill).
//   - resolveLineHeightPx path: set ta.style.lineHeight = '22.95px' so the
//     function reads it from getComputedStyle (jsdom honours inline styles).

describe('isOnLastVisualLine — LAST_LINE_TOLERANCE regression (P2.5b Fix A)', () => {
    it('returns true when markerOffsetTop=4, lineHeight=22.95px, scrollHeight=27 (integer rounding case)', () => {
        // Arrange: textarea with single-line content and inline lineHeight.
        const el = ta('Hello', 5);
        el.style.lineHeight = '22.95px';
        el.style.fontSize = '18px';

        // Mock offsetTop getter: first call (marker.offsetTop) returns 4; rest 0.
        let offsetTopCalls = 0;
        const offsetTopSpy = vi.spyOn(HTMLElement.prototype, 'offsetTop', 'get')
            .mockImplementation(function (this: HTMLElement) {
                offsetTopCalls++;
                // The first offsetTop read inside isOnLastVisualLine is for the marker span.
                return offsetTopCalls === 1 ? 4 : 0;
            });

        // Mock scrollHeight getter: first call (mirror.scrollHeight) returns 27.
        let scrollHeightCalls = 0;
        const scrollHeightSpy = vi.spyOn(Element.prototype, 'scrollHeight', 'get')
            .mockImplementation(function (this: Element) {
                scrollHeightCalls++;
                return scrollHeightCalls === 1 ? 27 : 0;
            });

        try {
            // markerOffsetTop=4, lineHeight=22.95, scrollHeight=27
            // Without LAST_LINE_TOLERANCE: 4 + 22.95 = 26.95 < 27 → false (bug)
            // With LAST_LINE_TOLERANCE=2:  4 + 22.95 + 2 = 28.95 >= 27 → true (fix)
            const result = isOnLastVisualLine(el);
            expect(result).toBe(true);
        } finally {
            offsetTopSpy.mockRestore();
            scrollHeightSpy.mockRestore();
        }
    });

    it('returns false when caret is a full line-height above the bottom (true negative)', () => {
        // Arrange: textarea where the caret is clearly not on the last visual row.
        // markerOffsetTop=4 (first row of a 2-row element), scrollHeight=50 (≈ 2 × 22.95).
        // With LAST_LINE_TOLERANCE=2: 4 + 22.95 + 2 = 28.95 < 50 → false (correct).
        const el = ta('Hello\nWorld', 5);
        el.style.lineHeight = '22.95px';
        el.style.fontSize = '18px';

        let offsetTopCalls = 0;
        const offsetTopSpy = vi.spyOn(HTMLElement.prototype, 'offsetTop', 'get')
            .mockImplementation(function (this: HTMLElement) {
                offsetTopCalls++;
                return offsetTopCalls === 1 ? 4 : 0;
            });

        let scrollHeightCalls = 0;
        const scrollHeightSpy = vi.spyOn(Element.prototype, 'scrollHeight', 'get')
            .mockImplementation(function (this: Element) {
                scrollHeightCalls++;
                return scrollHeightCalls === 1 ? 50 : 0;
            });

        try {
            const result = isOnLastVisualLine(el);
            expect(result).toBe(false);
        } finally {
            offsetTopSpy.mockRestore();
            scrollHeightSpy.mockRestore();
        }
    });
});

// ── B. Mockability — visual-line functions are standalone exports ──────────────
//
// P2.4b will inject or mock isOnFirstVisualLine / isOnLastVisualLine via
// vi.spyOn(caretGeometry, 'isOnFirstVisualLine') in its RTL tests (real
// browser layout is not available in RTL/jsdom either).
//
// This test verifies that the two functions CAN be spied on (i.e. they are
// true named exports, not closures wrapped at call site):

describe('visual-line functions are mockable (vi.spyOn)', () => {
    it('isOnFirstVisualLine can be intercepted with vi.spyOn', async () => {
        const mod = await import('./caretGeometry');
        const spy = vi.spyOn(mod, 'isOnFirstVisualLine').mockReturnValue(false);
        const el = ta('test', 0);
        const result = mod.isOnFirstVisualLine(el);
        expect(result).toBe(false);
        expect(spy).toHaveBeenCalledOnce();
        spy.mockRestore();
    });

    it('isOnLastVisualLine can be intercepted with vi.spyOn', async () => {
        const mod = await import('./caretGeometry');
        const spy = vi.spyOn(mod, 'isOnLastVisualLine').mockReturnValue(true);
        const el = ta('test', 0);
        const result = mod.isOnLastVisualLine(el);
        expect(result).toBe(true);
        expect(spy).toHaveBeenCalledOnce();
        spy.mockRestore();
    });
});

// ── C. prefixWidth (§2 caret-aware nest: bidirectional ancestor-prefix width) ──
//
// `prefixWidth(sourceLine, surfaceR0, content, map, cleanBuffer)` returns how many
// leading characters (the ancestor `> `/indent prefix) the clean nesting buffer
// strips from the FULL source line, so the §2 caret math can convert between
// buffer columns (textarea-native) and source columns:
//   Cs           = currentBufferCol + prefixWidth(current, Ls, …)   (read)
//   destBufferCol = Cs            − prefixWidth(dest,    Ls, …)      (place)
//
// Column space is UTF-16 (matches getLogicalColumn / placeCaretAtColumn). Byte
// offsets only feed sliceUtf8 / the line map. (Reflection #19.)
describe('prefixWidth', () => {
    it('returns 0 for a verbatim parent (cleanBuffer undefined)', () => {
        // A top-level surface has no stripped buffer → the clean line IS the source
        // line → no prefix.
        const content = 'plain text\n';
        const map = buildByteLineMap(content);
        expect(prefixWidth(0, 0, content, map, undefined)).toBe(0);
    });

    it('measures a blockquote `> ` prefix (2 chars) on every line', () => {
        const content = '> hello\n> world\n';
        const map = buildByteLineMap(content);
        const clean = 'hello\nworld'; // stripped buffer
        expect(prefixWidth(0, 0, content, map, clean)).toBe(2);
        expect(prefixWidth(1, 0, content, map, clean)).toBe(2);
    });

    it('measures a list indent prefix relative to a non-zero surface start line', () => {
        // The real 3-level fixture's level-1 list surface [20,69] spans source
        // lines 2–4. Its clean buffer strips the 4-space base indent off each line.
        const content =
            '* another\n* hello\n    * sub-item\n        * sub-sub-item\n    * nother\n';
        const map = buildByteLineMap(content);
        // map.lineOf(20) === 2, so bufferLine = sourceLine − 2.
        const clean = '* sub-item\n    * sub-sub-item\n* nother';
        expect(prefixWidth(2, 20, content, map, clean)).toBe(4); // "    * sub-item"
        expect(prefixWidth(3, 20, content, map, clean)).toBe(4); // 8-space line, 4 stripped
        expect(prefixWidth(4, 20, content, map, clean)).toBe(4); // "    * nother"
    });

    it('handles the last source line when content has NO trailing newline', () => {
        // The fail-on-revert lever for the lineStart(L+1)==lineStart(L) edge: without
        // slicing to end-of-content on the final line, fullT would be "" → guard
        // fails → wrong (negative) width.
        const content = '> hello\n> world'; // no trailing \n
        const map = buildByteLineMap(content);
        const clean = 'hello\nworld';
        expect(prefixWidth(1, 0, content, map, clean)).toBe(2);
    });

    it('uses UTF-16 column units, not byte counts, for the width', () => {
        // "é" is 2 UTF-8 bytes but 1 UTF-16 unit. The `> ` prefix is 2 columns
        // regardless of the multibyte content after it.
        const content = '> café\n';
        const map = buildByteLineMap(content);
        const clean = 'café';
        expect(prefixWidth(0, 0, content, map, clean)).toBe(2);
    });

    it('warns and returns the best-effort length delta when the clean line is not a suffix (inline normalization)', () => {
        // When a writer normalizes inline markup the clean line is no longer a
        // suffix of the source line; there is no exact integer prefix. We warn and
        // return the length delta; placeCaretAtColumn's column clamp absorbs any
        // overflow (clamp-and-warn posture).
        const content = '> *hi*\n';
        const map = buildByteLineMap(content);
        const clean = '_hi_'; // normalized emphasis marker — not a suffix of "> *hi*"
        const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
        try {
            expect(prefixWidth(0, 0, content, map, clean)).toBe(2); // 6 − 4, best effort
            expect(warn).toHaveBeenCalled();
        } finally {
            warn.mockRestore();
        }
    });

    it('returns 0 defensively when the buffer line index is out of range', () => {
        const content = '> hello\n';
        const map = buildByteLineMap(content);
        const clean = 'hello';
        // sourceLine 5 → bufferLine 5, but clean has 1 line → out of range → 0.
        expect(prefixWidth(5, 0, content, map, clean)).toBe(0);
    });
});
