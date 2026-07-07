// @vitest-environment jsdom
/**
 * Unit tests for classifyOpenSelection (bd-abo9m23f).
 *
 * When a mouse click/drag activates a block, the opening DOM selection is
 * classified into one of three outcomes:
 *  - a non-collapsed selection fully inside the activated block → `range`
 *    payload carrying direction-aware anchor/head viewport coords;
 *  - a non-collapsed selection NOT contained in the block (cross-block drag)
 *    → `'suppress'` (the activation is aborted, preserving the selection);
 *  - anything else (collapsed, no ranges, unreadable endpoint geometry) →
 *    `caret` payload with the click coords (the bd-q9lyghv2 behavior).
 *
 * jsdom has no layout, so endpoint rects (collapsed Range.getClientRects) are
 * mocked: x = startOffset * 10, line top 100, height 20. Real geometry is
 * covered by the Playwright e2e drag test.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { classifyOpenSelection } from './dragSelectionCapture';

let block: HTMLParagraphElement;
let otherBlock: HTMLParagraphElement;
let blockText: Text;
let otherText: Text;

const CLICK = { x: 400, y: 110 };

const caretRect = (offset: number): DOMRect =>
    ({
        left: offset * 10,
        right: offset * 10,
        top: 100,
        bottom: 120,
        height: 20,
        width: 0,
        x: offset * 10,
        y: 100,
        toJSON: () => ({}),
    }) as DOMRect;

// jsdom does not implement Range client-rect geometry (spyOn has nothing to
// hook), so install the synthetic implementations with defineProperty and
// restore the original descriptors afterwards. `rectsImpl` is swappable
// per-test (the "no rects" case).
let rectsImpl: (r: Range) => DOMRect[];
const savedDescriptors: Array<[string, PropertyDescriptor | undefined]> = [];

beforeEach(() => {
    document.body.innerHTML = '';
    block = document.createElement('p');
    block.setAttribute('data-block-pool-id', '5');
    blockText = document.createTextNode('the quick brown fox jumps');
    block.appendChild(blockText);
    otherBlock = document.createElement('p');
    otherBlock.setAttribute('data-block-pool-id', '6');
    otherText = document.createTextNode('another paragraph entirely');
    otherBlock.appendChild(otherText);
    document.body.append(block, otherBlock);

    // Synthetic endpoint geometry: a collapsed range at offset N in a text
    // node reports a caret rect at x = N * 10 on a line spanning y 100..120.
    rectsImpl = (r) => [caretRect(r.startOffset)];
    for (const name of ['getClientRects', 'getBoundingClientRect'] as const) {
        savedDescriptors.push([
            name,
            Object.getOwnPropertyDescriptor(Range.prototype, name),
        ]);
        Object.defineProperty(Range.prototype, name, {
            value: function (this: Range) {
                const rects = rectsImpl(this);
                return name === 'getClientRects'
                    ? (rects as unknown as DOMRectList)
                    : (rects[0] ??
                          ({ left: 0, right: 0, top: 0, bottom: 0, height: 0,
                             width: 0, x: 0, y: 0, toJSON: () => ({}) } as DOMRect));
            },
            configurable: true,
            writable: true,
        });
    }
});

afterEach(() => {
    for (const [name, desc] of savedDescriptors.splice(0)) {
        if (desc) Object.defineProperty(Range.prototype, name, desc);
        else delete (Range.prototype as any)[name];
    }
    vi.restoreAllMocks();
});

/** Install a fake window.getSelection reporting the given endpoints. The
 *  normalized range (start = earlier endpoint) is derived for containment. */
function setSelection(
    anchorNode: Node,
    anchorOffset: number,
    focusNode: Node,
    focusOffset: number,
    opts: { collapsed?: boolean; rangeCount?: number } = {},
) {
    const backward =
        anchorNode === focusNode
            ? focusOffset < anchorOffset
            : !!(
                  anchorNode.compareDocumentPosition(focusNode) &
                  Node.DOCUMENT_POSITION_PRECEDING
              );
    const [startContainer, endContainer] = backward
        ? [focusNode, anchorNode]
        : [anchorNode, focusNode];
    const sel = {
        isCollapsed: opts.collapsed ?? false,
        rangeCount: opts.rangeCount ?? 1,
        anchorNode,
        anchorOffset,
        focusNode,
        focusOffset,
        getRangeAt: () => ({ startContainer, endContainer }),
    };
    vi.spyOn(window, 'getSelection').mockReturnValue(sel as unknown as Selection);
}

describe('classifyOpenSelection', () => {
    it('returns a caret payload with the click coords when there is no selection', () => {
        vi.spyOn(window, 'getSelection').mockReturnValue(null);
        expect(classifyOpenSelection(block, CLICK)).toEqual({
            kind: 'caret',
            head: CLICK,
        });
    });

    it('returns a caret payload when the selection is collapsed (plain click)', () => {
        setSelection(blockText, 4, blockText, 4, { collapsed: true });
        expect(classifyOpenSelection(block, CLICK)).toEqual({
            kind: 'caret',
            head: CLICK,
        });
    });

    it('returns a caret payload when the selection has no ranges', () => {
        setSelection(blockText, 0, blockText, 5, { rangeCount: 0 });
        expect(classifyOpenSelection(block, CLICK)).toEqual({
            kind: 'caret',
            head: CLICK,
        });
    });

    it('maps a contained forward drag to a range payload (anchor before head)', () => {
        // "quick" (offset 4) dragged to after "fox" (offset 19).
        setSelection(blockText, 4, blockText, 19);
        expect(classifyOpenSelection(block, CLICK)).toEqual({
            kind: 'range',
            anchor: { x: 40, y: 110 },
            head: { x: 190, y: 110 },
        });
    });

    it('preserves direction for a backward drag (anchor after head)', () => {
        // Dragged right-to-left: anchor at 19, focus (release point) at 4.
        setSelection(blockText, 19, blockText, 4);
        expect(classifyOpenSelection(block, CLICK)).toEqual({
            kind: 'range',
            anchor: { x: 190, y: 110 },
            head: { x: 40, y: 110 },
        });
    });

    it("suppresses activation when the selection crosses out of the block", () => {
        // Starts in the activated block, ends in the next one.
        setSelection(blockText, 4, otherText, 7);
        expect(classifyOpenSelection(block, CLICK)).toBe('suppress');
    });

    it('suppresses activation when the selection lives entirely in ANOTHER block', () => {
        // Drag happened in block A; release (activation) landed on block B.
        setSelection(otherText, 0, otherText, 7);
        expect(classifyOpenSelection(block, CLICK)).toBe('suppress');
    });

    it('falls back to caret when endpoint geometry is unreadable (no rects)', () => {
        setSelection(blockText, 4, blockText, 19);
        rectsImpl = () => [];
        expect(classifyOpenSelection(block, CLICK)).toEqual({
            kind: 'caret',
            head: CLICK,
        });
    });

    it('falls back to caret when an endpoint offset is invalid for its node', () => {
        // setStart throws (offset beyond node length) → unreadable geometry.
        setSelection(blockText, 4, blockText, 9999);
        expect(classifyOpenSelection(block, CLICK)).toEqual({
            kind: 'caret',
            head: CLICK,
        });
    });
});
