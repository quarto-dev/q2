/**
 * Unit tests for placeCaretFromClick (bd-q9lyghv2).
 *
 * Pure logic over a fake editor: posAtCoords resolves the document position from
 * viewport coordinates; on a hit we move the selection there and focus; on a miss
 * we do nothing and report false so the caller can keep its end-of-block default.
 *
 * Real geometry (posAtCoords against laid-out DOM) is verified end-to-end in a
 * browser — jsdom returns null/0 — so here we mock the view.
 */

import { describe, it, expect, vi } from 'vitest';
import { EditorState } from '@tiptap/pm/state';
import { placeCaretFromClick, placeSelectionFromDrag } from './caretFromClick';
import { richTextSchema } from './schema';

/** Build a fake tiptap editor recording the chained caret commands. */
function fakeEditor(posAtCoordsReturn: { pos: number; inside: number } | null) {
    const calls = { setTextSelection: [] as number[], focus: 0, run: 0 };
    const chain: any = {
        focus() { calls.focus++; return chain; },
        setTextSelection(pos: number) { calls.setTextSelection.push(pos); return chain; },
        run() { calls.run++; return true; },
    };
    const editor = {
        view: { posAtCoords: vi.fn().mockReturnValue(posAtCoordsReturn) },
        chain: () => chain,
    };
    return { editor, calls };
}

describe('placeCaretFromClick', () => {
    it('maps coords with posAtCoords and moves the selection to the hit pos', () => {
        const { editor, calls } = fakeEditor({ pos: 7, inside: 0 });

        const placed = placeCaretFromClick(editor as any, { x: 42, y: 117 });

        expect(placed).toBe(true);
        // posAtCoords takes {left, top} (ProseMirror's coordinate shape).
        expect(editor.view.posAtCoords).toHaveBeenCalledWith({ left: 42, top: 117 });
        expect(calls.setTextSelection).toEqual([7]);
        expect(calls.focus).toBeGreaterThanOrEqual(1);
        expect(calls.run).toBe(1);
    });

    it('returns false and moves nothing when posAtCoords misses (null)', () => {
        const { editor, calls } = fakeEditor(null);

        const placed = placeCaretFromClick(editor as any, { x: 1, y: 1 });

        expect(placed).toBe(false);
        expect(calls.setTextSelection).toEqual([]);
        expect(calls.run).toBe(0);
    });
});

/**
 * placeSelectionFromDrag (bd-abo9m23f) — replay a drag selection's two
 * endpoints. Uses a REAL EditorState (richTextSchema, one paragraph) so the
 * TextSelection the helper builds is validated against real resolve/between
 * semantics; only the view's coordinate lookup and dispatch are faked.
 */

/** doc: <p>hello world drag me</p> — inline positions 1..19. */
function fakeDragEditor(coordToPos: Map<string, number | null>) {
    const doc = richTextSchema.node('doc', null, [
        richTextSchema.node('paragraph', null, [
            richTextSchema.text('hello world drag me'),
        ]),
    ]);
    let state = EditorState.create({ doc });
    const calls = { dispatch: 0, focus: 0 };
    const view = {
        get state() { return state; },
        posAtCoords(pt: { left: number; top: number }) {
            const pos = coordToPos.get(`${pt.left},${pt.top}`);
            return pos == null ? null : { pos, inside: 0 };
        },
        dispatch(tr: any) { calls.dispatch++; state = state.apply(tr); },
    };
    const editor = {
        view,
        commands: { focus: () => { calls.focus++; return true; } },
    };
    return { editor, calls, getSelection: () => state.selection };
}

describe('placeSelectionFromDrag', () => {
    it('maps both endpoints and sets a forward range selection (direction preserved)', () => {
        const { editor, calls, getSelection } = fakeDragEditor(
            new Map([['10,100', 3], ['80,100', 12]]),
        );

        const placed = placeSelectionFromDrag(
            editor as any,
            { x: 10, y: 100 },   // anchor
            { x: 80, y: 100 },   // head
        );

        expect(placed).toBe(true);
        expect(calls.dispatch).toBe(1);
        expect(calls.focus).toBeGreaterThanOrEqual(1);
        expect(getSelection().anchor).toBe(3);
        expect(getSelection().head).toBe(12);
    });

    it('preserves a backward drag (anchor > head)', () => {
        const { editor, getSelection } = fakeDragEditor(
            new Map([['80,100', 12], ['10,100', 3]]),
        );

        const placed = placeSelectionFromDrag(
            editor as any,
            { x: 80, y: 100 },   // anchor (drag started here)
            { x: 10, y: 100 },   // head (released here, to the left)
        );

        expect(placed).toBe(true);
        expect(getSelection().anchor).toBe(12);
        expect(getSelection().head).toBe(3);
        expect(getSelection().empty).toBe(false);
    });

    it('returns false and moves nothing when the anchor misses', () => {
        const { editor, calls } = fakeDragEditor(new Map([['80,100', 12]]));

        const placed = placeSelectionFromDrag(
            editor as any,
            { x: 10, y: 100 },
            { x: 80, y: 100 },
        );

        expect(placed).toBe(false);
        expect(calls.dispatch).toBe(0);
    });

    it('returns false and moves nothing when the head misses', () => {
        const { editor, calls } = fakeDragEditor(new Map([['10,100', 3]]));

        const placed = placeSelectionFromDrag(
            editor as any,
            { x: 10, y: 100 },
            { x: 80, y: 100 },
        );

        expect(placed).toBe(false);
        expect(calls.dispatch).toBe(0);
    });

    it('returns false when both endpoints resolve to the same position (degenerate)', () => {
        // Caller falls back to placeCaretFromClick — same visible outcome,
        // one code path for the caret case.
        const { editor, calls } = fakeDragEditor(
            new Map([['10,100', 5], ['11,100', 5]]),
        );

        const placed = placeSelectionFromDrag(
            editor as any,
            { x: 10, y: 100 },
            { x: 11, y: 100 },
        );

        expect(placed).toBe(false);
        expect(calls.dispatch).toBe(0);
    });
});
