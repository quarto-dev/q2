/**
 * RichTextEditor opening-selection consume wiring (bd-q9lyghv2, bd-abo9m23f).
 *
 * At mount the editor must read-and-clear `pendingOpenSelectionRef`, then
 * (one frame later) apply the payload with a fallback chain:
 *   range payload → placeSelectionFromDrag; on miss ↓
 *   head coords   → placeCaretFromClick (caret at the release point); on miss ↓
 *   no payload    → end-of-block focus (historical default).
 * Either way the ref is consumed exactly once so a re-anchor remount cannot
 * replay stale coordinates.
 *
 * Real posAtCoords geometry is browser-verified — here we mock the helpers to
 * observe the wiring (what was called, with what, and is the ref cleared?).
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import React, { useRef } from 'react';
import { PreviewContext } from '../PreviewContext';
import type { PreviewContextValue } from '../PreviewContext';
import type { ResolvedSource } from '../sourceIndex';
import type { PendingOpenSelection } from '../dragSelectionCapture';
import { RichTextEditor } from './RichTextEditor';

// Observe the placement calls without needing real layout/geometry.
const placeCaretFromClick = vi.fn().mockReturnValue(true);
const placeSelectionFromDrag = vi.fn().mockReturnValue(true);
vi.mock('./caretFromClick', () => ({
    placeCaretFromClick: (...args: unknown[]) => placeCaretFromClick(...args),
    placeSelectionFromDrag: (...args: unknown[]) => placeSelectionFromDrag(...args),
}));

afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    placeCaretFromClick.mockReturnValue(true);
    placeSelectionFromDrag.mockReturnValue(true);
});

// A single "hello" paragraph: pool[0] spans the whole 5-byte source.
const CONTENT = 'hello';
const POOL = [{ t: 0, r: [0, 5] as [number, number], d: 0 }];
const SOURCE_NODE = { t: 'Para', c: [{ t: 'Str', c: 'hello' }], s: 0 } as unknown;
const RESOLVED: ResolvedSource = {
    sourceNode: SOURCE_NODE as ResolvedSource['sourceNode'],
    reachabilityClass: 'reachable' as ResolvedSource['reachabilityClass'],
    sourceEntry: { t: 0, r: [0, 5], d: 0 },
};

/** Mount RichTextEditor with a pre-seeded payload ref; expose it to the test. */
function mountEditor(initialPayload: PendingOpenSelection | null) {
    let payloadRef!: React.MutableRefObject<PendingOpenSelection | null>;
    function Host() {
        payloadRef = useRef<PendingOpenSelection | null>(initialPayload);
        const editDraftRef = useRef<string | null>(CONTENT);
        const ctx: PreviewContextValue = {
            currentFilePath: '/project/test.qmd',
            pool: POOL as PreviewContextValue['pool'],
            content: CONTENT,
            editDraftRef,
            pendingOpenSelectionRef: payloadRef,
            setEditTarget: vi.fn(),
        };
        return (
            <PreviewContext.Provider value={ctx}>
                <RichTextEditor ctx={ctx} resolved={RESOLVED} />
            </PreviewContext.Provider>
        );
    }
    const utils = render(<Host />);
    return { ...utils, getPayload: () => payloadRef.current };
}

/** Flush a requestAnimationFrame tick (placement is deferred one frame so the
 *  swapped-in editor box is laid out before posAtCoords reads its geometry). */
const nextFrame = () =>
    new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

describe('RichTextEditor — opening-selection consume', () => {
    it('consumes a caret payload at mount and places the caret (next frame)', async () => {
        const { getPayload } = mountEditor({ kind: 'caret', head: { x: 42, y: 117 } });

        // The ref is consumed synchronously at mount (so a remount can't reuse a
        // stale click); placement itself is deferred one frame.
        expect(getPayload()).toBeNull();
        expect(placeCaretFromClick).not.toHaveBeenCalled();

        await nextFrame();

        expect(placeCaretFromClick).toHaveBeenCalledTimes(1);
        expect(placeCaretFromClick.mock.calls[0][1]).toEqual({ x: 42, y: 117 });
        expect(placeSelectionFromDrag).not.toHaveBeenCalled();
    });

    it('consumes a range payload and replays both endpoints (next frame)', async () => {
        const { getPayload } = mountEditor({
            kind: 'range',
            anchor: { x: 10, y: 100 },
            head: { x: 80, y: 100 },
        });

        expect(getPayload()).toBeNull();
        await nextFrame();

        expect(placeSelectionFromDrag).toHaveBeenCalledTimes(1);
        expect(placeSelectionFromDrag.mock.calls[0][1]).toEqual({ x: 10, y: 100 });
        expect(placeSelectionFromDrag.mock.calls[0][2]).toEqual({ x: 80, y: 100 });
        // Range replay succeeded → no caret fallback.
        expect(placeCaretFromClick).not.toHaveBeenCalled();
    });

    it('falls back to a caret at the head when the range replay misses', async () => {
        placeSelectionFromDrag.mockReturnValue(false);
        mountEditor({
            kind: 'range',
            anchor: { x: 10, y: 100 },
            head: { x: 80, y: 100 },
        });

        await nextFrame();

        expect(placeSelectionFromDrag).toHaveBeenCalledTimes(1);
        // Head = release point — the bd-q9lyghv2 caret behavior.
        expect(placeCaretFromClick).toHaveBeenCalledTimes(1);
        expect(placeCaretFromClick.mock.calls[0][1]).toEqual({ x: 80, y: 100 });
    });

    it('places nothing (end-of-block default) when no payload is present', async () => {
        const { getPayload } = mountEditor(null);
        await nextFrame();

        expect(placeCaretFromClick).not.toHaveBeenCalled();
        expect(placeSelectionFromDrag).not.toHaveBeenCalled();
        expect(getPayload()).toBeNull();
    });
});
