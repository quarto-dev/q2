/**
 * P2.4b integration tests: cross-surface arrow-navigation move machine.
 *
 * TDD: these tests are written BEFORE implementation. Run them first to see
 * them fail, then implement, then verify they pass.
 *
 * Coverage:
 *
 *  1. captureEditTarget helper — extraction of identity from a tile element.
 *
 *  2. Cross-surface nav trigger (mocked geometry):
 *     - ArrowDown on last visual line → hop to next tile (unmodified, no commit)
 *     - ArrowUp on first visual line → hop to previous tile (unmodified, no commit)
 *     - Wrap: ArrowDown on last tile → first tile; ArrowUp on first tile → last tile
 *     - Arrow NOT on edge → no hop (native caret move)
 *     - Modifier+Arrow at edge → no hop (Shift/Ctrl/Alt/Meta)
 *     - Single-tile document → no-op (ArrowDown/Up at edge does nothing)
 *
 *  3. Trigger robustness:
 *     - Unmodified move → synchronous hop, commitTextEdit NOT called, destination
 *       textarea opens in the same tick (no editability gap)
 *     - Modified move → commitTextEdit called + setEditTarget(null) + pendingLanding
 *       stashed; on re-render (new props) → destination editor opens (reland)
 *     - Byte-identical-commit fallback: dirty edit produces byte-identical output
 *       (props unchanged) → timeout fallback still relands (fake timers)
 *     - File switch cancels pending land (fromFile !== currentFilePath)
 *
 *  4. Caret on arrival:
 *     - After ArrowDown move, destination textarea.selectionStart is on first
 *       logical line at min(exitColumn, lineLen)
 *     - After ArrowUp move, destination textarea.selectionStart is on last
 *       logical line at min(exitColumn, lineLen)
 *
 * Geometry notes:
 *   - isOnFirstVisualLine / isOnLastVisualLine are mocked via vi.spyOn because
 *     jsdom provides no real layout (they always return true there).
 *   - Real-browser geometry + Playwright scenario are deferred to P2.5.
 */

// @vitest-environment jsdom

import { describe, it, expect, vi, afterEach } from 'vitest';
import {
    render,
    cleanup,
    act,
    fireEvent,
} from '@testing-library/react';
import React, {
    useState,
    useRef,
    useCallback,
} from 'react';
import { PreviewContext } from './PreviewContext';
import type { PreviewContextValue } from './PreviewContext';
import { RegistryContext } from '../framework';
import { Block } from './dispatchers';
import { captureEditTarget, measureTileBox } from './lockedTiles';
import * as caretGeometry from './caretGeometry';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.useRealTimers();
});

/* ─── shared types ──────────────────────────────────────────────────────────── */

type EditTarget = NonNullable<PreviewContextValue['editTarget']>;

interface PoolEntry {
    t: 0;
    r: [number, number];
    d: 0;
}

const MOCK_BOX_STYLE: Record<string, string> = {
    marginTop: '0px', marginRight: '0px', marginBottom: '0px', marginLeft: '0px',
    paddingTop: '0px', paddingRight: '0px', paddingBottom: '0px', paddingLeft: '0px',
    borderTopWidth: '0px', borderRightWidth: '0px', borderBottomWidth: '0px', borderLeftWidth: '0px',
    borderTopStyle: 'none', borderRightStyle: 'none', borderBottomStyle: 'none', borderLeftStyle: 'none',
    borderTopColor: 'rgb(0,0,0)', borderRightColor: 'rgb(0,0,0)',
    borderBottomColor: 'rgb(0,0,0)', borderLeftColor: 'rgb(0,0,0)',
};

function makeEditTarget(r0: number, r1: number, slice: string): EditTarget {
    return {
        anchorR0: r0,
        anchorR1: r1,
        anchorSlice: slice,
        contentHeight: 40,
        boxStyle: MOCK_BOX_STYLE,
    };
}

/**
 * A tile element with a visible rect mock.
 * Uses useLayoutEffect so mock is installed before any parent layout effects.
 */
function TileElement({
    poolId,
    visible = true,
    tabIndex = -1,
}: {
    poolId: number;
    visible?: boolean;
    tabIndex?: number;
}) {
    const ref = React.useRef<HTMLParagraphElement>(null);
    React.useLayoutEffect(() => {
        if (ref.current) {
            vi.spyOn(ref.current, 'getBoundingClientRect').mockReturnValue(
                visible
                    ? { left: 0, top: poolId * 50, right: 200, bottom: poolId * 50 + 40, width: 200, height: 40, x: 0, y: poolId * 50, toJSON: () => ({}) }
                    : { left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0, x: 0, y: 0, toJSON: () => ({}) },
            );
        }
    });
    return (
        <p
            ref={ref}
            data-block-pool-id={String(poolId)}
            data-testid={`tile-${poolId}`}
            tabIndex={tabIndex}
        />
    );
}

/**
 * Pending landing descriptor (what PreviewRoot would hold in pendingLandingRef).
 */
interface PendingLanding {
    direction: 'up' | 'down';
    destLine: number;
    desiredColumn: number;
    fromFile: string;
}

/**
 * Pending caret hint (what EditTextarea reads on mount).
 */
interface PendingCaret {
    edge: 'first' | 'last';
    column: number;
}

/* ─── 1. captureEditTarget helper ────────────────────────────────────────────── */

describe('captureEditTarget', () => {
    it('returns the identity triple from a tile element and pool', () => {
        // Pool entry 0: r=[0, 11]
        const pool: unknown[] = [
            { t: 0, r: [0, 11], d: 0 },
        ];
        const content = 'hello world'; // 11 ASCII bytes

        // Build a tile element with data-block-pool-id="0"
        const div = document.createElement('div');
        div.setAttribute('data-block-pool-id', '0');
        document.body.appendChild(div);

        try {
            const result = captureEditTarget(div, pool, content);
            expect(result).not.toBeNull();
            expect(result!.anchorR0).toBe(0);
            expect(result!.anchorR1).toBe(11);
            expect(result!.anchorSlice).toBe('hello world');
        } finally {
            document.body.removeChild(div);
        }
    });

    it('returns null when the element has no data-block-pool-id', () => {
        const div = document.createElement('div');
        document.body.appendChild(div);
        try {
            const result = captureEditTarget(div, [], '');
            expect(result).toBeNull();
        } finally {
            document.body.removeChild(div);
        }
    });

    it('returns null for a non-Original pool entry (t !== 0)', () => {
        const pool: unknown[] = [
            { t: 1, r: [0, 5], d: 0 }, // Generated
        ];
        const div = document.createElement('div');
        div.setAttribute('data-block-pool-id', '0');
        document.body.appendChild(div);
        try {
            const result = captureEditTarget(div, pool, '');
            expect(result).toBeNull();
        } finally {
            document.body.removeChild(div);
        }
    });

    it('normalizes line endings and trims in anchorSlice', () => {
        const pool: unknown[] = [
            { t: 0, r: [0, 8], d: 0 }, // covers "foo\r\n  " (8 bytes) → normalizes+trims to "foo"
        ];
        const content = 'foo\r\n   rest';
        const div = document.createElement('div');
        div.setAttribute('data-block-pool-id', '0');
        document.body.appendChild(div);
        try {
            const result = captureEditTarget(div, pool, content);
            expect(result).not.toBeNull();
            // "foo\r\n  " normalized is "foo\n  ", trimEnd() is "foo\n" wait... trimEnd trims trailing whitespace
            // "foo\r\n  " → normalize → "foo\n  " → trimEnd → "foo"
            // Actually: sliceBytes(content, 0, 8) = "foo\r\n   " (8 bytes: f,o,o,\r,\n, , , )
            // normalizeLineEndings("foo\r\n   ").trimEnd() = "foo\n   ".trimEnd() = "foo"
            expect(result!.anchorSlice).toBe('foo');
        } finally {
            document.body.removeChild(div);
        }
    });
});

/* ─── Fix 1: measureTileBox ──────────────────────────────────────────────────── */

describe('measureTileBox', () => {
    it('returns contentHeight=0 and empty-ish boxStyle for an unmeasured jsdom element', () => {
        // In jsdom getBoundingClientRect always returns zero rect; getComputedStyle
        // returns empty/zero strings. measureTileBox should still return a populated
        // boxStyle object (all 20 keys present) with contentHeight=0.
        const div = document.createElement('div');
        document.body.appendChild(div);
        try {
            const result = measureTileBox(div);
            // contentHeight = 0 - 0 - 0 - 0 - 0 = 0
            expect(result.contentHeight).toBe(0);
            // boxStyle has all 20 expected longhands
            expect(Object.keys(result.boxStyle)).toHaveLength(20);
            expect('marginTop' in result.boxStyle).toBe(true);
            expect('borderTopColor' in result.boxStyle).toBe(true);
        } finally {
            document.body.removeChild(div);
        }
    });

    it('returns non-zero contentHeight when getBoundingClientRect is mocked with a real rect', () => {
        const div = document.createElement('div');
        document.body.appendChild(div);
        try {
            // Mock a 200×80 element with 10px padding top/bottom and 2px border top/bottom
            vi.spyOn(div, 'getBoundingClientRect').mockReturnValue(
                { left: 0, top: 0, right: 200, bottom: 80, width: 200, height: 80, x: 0, y: 0, toJSON: () => ({}) },
            );
            // getComputedStyle returns strings; we can't easily mock CSSStyleDeclaration
            // properties directly — but we can verify the formula:
            // contentHeight = 80 - paddingTop - paddingBottom - borderTopWidth - borderBottomWidth
            // In jsdom these are all '', parseFloat('') = NaN → NaN becomes 0 via subtraction
            // So contentHeight = 80 - 0 - 0 - 0 - 0 = 80.
            const result = measureTileBox(div);
            expect(result.contentHeight).toBe(80);
            expect(Object.keys(result.boxStyle)).toHaveLength(20);
        } finally {
            document.body.removeChild(div);
        }
    });
});

/* ─── helpers for multi-tile documents ─────────────────────────────────────── */

/**
 * Three-tile document:
 *   tile 0: pool[0] r=[0, 6]   → "para0\n"  → line 0
 *   tile 1: pool[1] r=[6, 13]  → "para1\n\n" → line 1
 *   tile 2: pool[2] r=[13, 20] → "para2\n\n" → line 3
 */
const THREE_TILE_CONTENT = 'para0\npara1\n\npara2\n\n';
const THREE_TILE_POOL: PoolEntry[] = [
    { t: 0, r: [0, 6], d: 0 },    // "para0\n" — line 0
    { t: 0, r: [6, 13], d: 0 },   // "para1\n\n" — line 1
    { t: 0, r: [13, 20], d: 0 },  // "para2\n\n" — line 3
];

/* ─── Fix 1: move-opened editor has real box measurement ─────────────────────── */

/**
 * BoxMeasureHarness: a minimal harness that exposes requestMove and provides
 * a PreviewContext with measureTileBox-capable tile elements (mocked rects).
 *
 * Verifies that after a synchronous hop, the opened editor's editTarget has a
 * non-zero contentHeight and a populated boxStyle — i.e. measureTileBox was
 * called on the destination tile and its result was used.
 */
function BoxMeasureHarness({
    onOpen,
    onReady,
    children,
}: {
    onOpen?: vi.Mock;
    onReady?: (fns: { requestMove: (...args: any[]) => void }) => void;
    children?: React.ReactNode;
}) {
    const pool: PoolEntry[] = THREE_TILE_POOL;
    const content = THREE_TILE_CONTENT;
    const hostRef = useRef<HTMLDivElement | null>(null);
    const editTargetRef = useRef<EditTarget | null>(makeEditTarget(0, 6, 'para0'));
    const editDraftRef = useRef<string | null>('para0');
    const pendingCaretRef = useRef<PendingCaret | null>(null);
    const pendingLandingRef = useRef<PendingLanding | null>(null);
    const poolRef = useRef(pool);
    const contentRef = useRef(content);

    const [editTarget, setEditTargetRaw] = React.useState<EditTarget | null>(makeEditTarget(0, 6, 'para0'));

    const setEditTarget = useCallback((target: EditTarget | null) => {
        editTargetRef.current = target;
        setEditTargetRaw(target);
        if (target !== null) {
            onOpen?.(target);
        }
    }, [onOpen]);

    const requestMove = useCallback((
        direction: 'down' | 'up',
        exitColumn: number,
        draft: string,
        isDirty: boolean,
        _sourceInfoJson: string,
    ) => {
        const et = editTargetRef.current;
        if (!et || !hostRef.current) return;
        const tiles = Array.from(hostRef.current.querySelectorAll<HTMLElement>('[data-block-pool-id]'))
            .filter(el => {
                const r = el.getBoundingClientRect();
                return r.width > 0 || r.height > 0;
            });
        if (tiles.length <= 1) return;

        // For simplicity in this test: go to tile 1 (pool[1])
        const destTile = tiles.find(t => t.getAttribute('data-block-pool-id') === '1') ?? tiles[1];
        if (!destTile || isDirty) return;

        const currentPool = poolRef.current;
        const currentContent = contentRef.current;
        const captured = captureEditTarget(destTile, currentPool, currentContent);
        if (!captured) return;

        editDraftRef.current = captured.anchorSlice;
        pendingCaretRef.current = { edge: direction === 'down' ? 'first' : 'last', column: exitColumn };
        // Use the real measureTileBox — this is the fix under test
        const { contentHeight, boxStyle } = measureTileBox(destTile);
        setEditTarget({ ...captured, contentHeight, boxStyle });
    }, [setEditTarget]);

    React.useEffect(() => {
        onReady?.({ requestMove });
    }, [onReady, requestMove]);

    const ctx: PreviewContextValue = {
        currentFilePath: '/test.qmd',
        pool,
        content,
        editTarget,
        setEditTarget,
        editDraftRef,
        pendingCaretRef,
        activeEditRegionRef: useRef<HTMLDivElement | null>(null),
        commitTextEdit: () => {},
        resolveSource: (node: any) => {
            const poolId = node.s;
            if (poolId === undefined) return null;
            const entry = pool[poolId] as PoolEntry | null | undefined;
            if (!entry || entry.t !== 0 || entry.d !== 0) return null;
            return { sourceNode: node, reachabilityClass: 'TopLevel' as const, sourceEntry: entry };
        },
        requestMove,
    };

    return (
        <PreviewContext.Provider value={ctx}>
            <RegistryContext.Provider value={{ registry: {} }}>
                <div ref={hostRef}>
                    {children}
                </div>
            </RegistryContext.Provider>
        </PreviewContext.Provider>
    );
}

describe('Fix 1 — move-opened editor has real box measurement (not height:0)', () => {
    it('sync-hop opens editor with non-zero contentHeight and populated boxStyle when dest tile rect is mocked', async () => {
        const onOpen = vi.fn();
        let moveRef: ((...args: any[]) => void) | null = null;
        const onReady = vi.fn(({ requestMove }: any) => { moveRef = requestMove; });

        render(
            <BoxMeasureHarness onOpen={onOpen} onReady={onReady}>
                <TileElement poolId={0} />
                <TileElement poolId={1} />
                <TileElement poolId={2} />
            </BoxMeasureHarness>,
        );

        await act(async () => {});
        onOpen.mockClear();

        // TileElement mocks getBoundingClientRect to return height=40 for visible tiles.
        // measureTileBox(tile1) should return contentHeight=40 (height - 0 - 0 - 0 - 0).
        await act(async () => {
            moveRef!('down', 0, 'para0', false, '{}');
        });

        expect(onOpen).toHaveBeenCalledOnce();
        const openedTarget = onOpen.mock.calls[0][0] as EditTarget;
        // The fix: contentHeight should be the mocked height (40), NOT 0.
        expect(openedTarget.contentHeight).toBe(40);
        // boxStyle should be a populated object (all 20 keys)
        expect(Object.keys(openedTarget.boxStyle)).toHaveLength(20);
        // The destination tile's anchorR0 should be 6 (pool[1])
        expect(openedTarget.anchorR0).toBe(6);
    });
});

/* ─── Fix 3: Real keydown through EditTextarea ──────────────────────────────── */

/**
 * EditTextareaKeydownHarness: mounts a real Block (which renders EditTextarea)
 * and wires a spy-wrapped requestMove into the PreviewContext.
 *
 * This is the harness that actually exercises EditTextarea.onKeyDown →
 * ctx.requestMove, which the previous tests did NOT cover (DirectMoveHarness
 * omitted requestMove from its ctx).
 */

// A minimal Para node so Block renders an EditTextarea when it matches editTarget.
function makeParaNode(poolId: number) {
    return { t: 'Para', c: [], s: poolId };
}

interface KeydownHarnessProps {
    pool: unknown[];
    content: string;
    editTarget: EditTarget;
    requestMove: vi.Mock;
    cancelPendingLand?: () => void;
}

function EditTextareaKeydownHarness({
    pool,
    content,
    editTarget,
    requestMove,
    cancelPendingLand,
}: KeydownHarnessProps) {
    const [et, setEt] = React.useState<EditTarget | null>(editTarget);
    const editDraftRef = useRef<string | null>(editTarget.anchorSlice);
    const pendingCaretRef = useRef<{ edge: 'first' | 'last'; column: number } | null>(null);

    const ctx: PreviewContextValue = {
        currentFilePath: '/test.qmd',
        pool,
        content,
        editTarget: et,
        setEditTarget: (target) => setEt(target),
        editDraftRef,
        pendingCaretRef,
        activeEditRegionRef: useRef<HTMLDivElement | null>(null),
        commitTextEdit: () => {},
        resolveSource: (node: any) => {
            const poolId = node.s;
            if (poolId === undefined) return null;
            const entry = pool[poolId] as PoolEntry | null | undefined;
            if (!entry || entry.t !== 0 || entry.d !== 0) return null;
            return { sourceNode: node, reachabilityClass: 'TopLevel' as const, sourceEntry: entry };
        },
        requestMove,
        cancelPendingLand,
    };

    return (
        <PreviewContext.Provider value={ctx}>
            <RegistryContext.Provider value={{ registry: {} }}>
                <Block node={makeParaNode(0) as any} />
            </RegistryContext.Provider>
        </PreviewContext.Provider>
    );
}

describe('Fix 3 — real keydown through EditTextarea.onKeyDown → requestMove', () => {
    it('ArrowDown on last visual line calls requestMove with direction=down', async () => {
        // Mock geometry: caret IS on the last visual line
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(false);

        const requestMove = vi.fn();
        const et = makeEditTarget(0, 6, 'para0');

        render(
            <EditTextareaKeydownHarness
                pool={THREE_TILE_POOL}
                content={THREE_TILE_CONTENT}
                editTarget={et}
                requestMove={requestMove}
            />,
        );

        const textarea = document.querySelector('textarea');
        expect(textarea).not.toBeNull();

        // Fire a bare ArrowDown (no modifiers)
        await act(async () => {
            fireEvent.keyDown(textarea!, { key: 'ArrowDown' });
        });

        // requestMove MUST be called (the edit textarea's onKeyDown → ctx.requestMove)
        expect(requestMove).toHaveBeenCalledOnce();
        const [direction] = requestMove.mock.calls[0];
        expect(direction).toBe('down');
    });

    it('ArrowDown NOT on last visual line does NOT call requestMove', async () => {
        // Mock geometry: caret is NOT on the last visual line
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(false);

        const requestMove = vi.fn();
        const et = makeEditTarget(0, 6, 'para0');

        render(
            <EditTextareaKeydownHarness
                pool={THREE_TILE_POOL}
                content={THREE_TILE_CONTENT}
                editTarget={et}
                requestMove={requestMove}
            />,
        );

        const textarea = document.querySelector('textarea');
        expect(textarea).not.toBeNull();

        await act(async () => {
            fireEvent.keyDown(textarea!, { key: 'ArrowDown' });
        });

        // NOT on the last visual line → requestMove must NOT be called
        expect(requestMove).not.toHaveBeenCalled();
    });

    it('ArrowUp on first visual line calls requestMove with direction=up', async () => {
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(false);

        const requestMove = vi.fn();
        const et = makeEditTarget(0, 6, 'para0');

        render(
            <EditTextareaKeydownHarness
                pool={THREE_TILE_POOL}
                content={THREE_TILE_CONTENT}
                editTarget={et}
                requestMove={requestMove}
            />,
        );

        const textarea = document.querySelector('textarea');
        expect(textarea).not.toBeNull();

        await act(async () => {
            fireEvent.keyDown(textarea!, { key: 'ArrowUp' });
        });

        expect(requestMove).toHaveBeenCalledOnce();
        const [direction] = requestMove.mock.calls[0];
        expect(direction).toBe('up');
    });

    it('Shift+ArrowDown at last visual line does NOT call requestMove (modifier guard)', async () => {
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);

        const requestMove = vi.fn();
        const et = makeEditTarget(0, 6, 'para0');

        render(
            <EditTextareaKeydownHarness
                pool={THREE_TILE_POOL}
                content={THREE_TILE_CONTENT}
                editTarget={et}
                requestMove={requestMove}
            />,
        );

        const textarea = document.querySelector('textarea');
        expect(textarea).not.toBeNull();

        // Shift+ArrowDown should NOT trigger a move (native selection behavior)
        await act(async () => {
            fireEvent.keyDown(textarea!, { key: 'ArrowDown', shiftKey: true });
        });

        expect(requestMove).not.toHaveBeenCalled();
    });

    it('Ctrl+ArrowDown at last visual line does NOT call requestMove (modifier guard)', async () => {
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);

        const requestMove = vi.fn();
        const et = makeEditTarget(0, 6, 'para0');

        render(
            <EditTextareaKeydownHarness
                pool={THREE_TILE_POOL}
                content={THREE_TILE_CONTENT}
                editTarget={et}
                requestMove={requestMove}
            />,
        );

        const textarea = document.querySelector('textarea');
        expect(textarea).not.toBeNull();

        await act(async () => {
            fireEvent.keyDown(textarea!, { key: 'ArrowDown', ctrlKey: true });
        });

        expect(requestMove).not.toHaveBeenCalled();
    });

    it('Esc calls cancelPendingLand and closes the editor', async () => {
        const requestMove = vi.fn();
        const cancelPendingLand = vi.fn();
        const et = makeEditTarget(0, 6, 'para0');

        render(
            <EditTextareaKeydownHarness
                pool={THREE_TILE_POOL}
                content={THREE_TILE_CONTENT}
                editTarget={et}
                requestMove={requestMove}
                cancelPendingLand={cancelPendingLand}
            />,
        );

        const textarea = document.querySelector('textarea');
        expect(textarea).not.toBeNull();

        await act(async () => {
            fireEvent.keyDown(textarea!, { key: 'Escape' });
        });

        // cancelPendingLand must have been called before the editor closes
        expect(cancelPendingLand).toHaveBeenCalledOnce();
        // requestMove must NOT be called on Esc
        expect(requestMove).not.toHaveBeenCalled();
    });
});

/* ─── Fix 4: Caret selectionStart on arrival ─────────────────────────────────── */

/**
 * CaretArrivalHarness: mounts a real EditTextarea (via Block) and populates
 * pendingCaretRef with a caret hint before the textarea mounts. After mount,
 * the useLayoutEffect in EditTextarea should call placeCaretAtColumn and the
 * textarea's selectionStart should be at the expected position.
 */

interface CaretArrivalHarnessProps {
    pool: unknown[];
    content: string;
    editTarget: EditTarget;
    pendingCaret: { edge: 'first' | 'last'; column: number };
}

function CaretArrivalHarness({
    pool,
    content,
    editTarget,
    pendingCaret,
}: CaretArrivalHarnessProps) {
    const [et] = React.useState<EditTarget | null>(editTarget);
    const editDraftRef = useRef<string | null>(editTarget.anchorSlice);
    // Pre-populate pendingCaretRef so EditTextarea's mount effect consumes it.
    const pendingCaretRef = useRef<{ edge: 'first' | 'last'; column: number } | null>(pendingCaret);

    const ctx: PreviewContextValue = {
        currentFilePath: '/test.qmd',
        pool,
        content,
        editTarget: et,
        setEditTarget: () => {},
        editDraftRef,
        pendingCaretRef,
        activeEditRegionRef: useRef<HTMLDivElement | null>(null),
        commitTextEdit: () => {},
        resolveSource: (node: any) => {
            const poolId = node.s;
            if (poolId === undefined) return null;
            const entry = pool[poolId] as PoolEntry | null | undefined;
            if (!entry || entry.t !== 0 || entry.d !== 0) return null;
            return { sourceNode: node, reachabilityClass: 'TopLevel' as const, sourceEntry: entry };
        },
    };

    return (
        <PreviewContext.Provider value={ctx}>
            <RegistryContext.Provider value={{ registry: {} }}>
                <Block node={makeParaNode(0) as any} />
            </RegistryContext.Provider>
        </PreviewContext.Provider>
    );
}

describe('Fix 4 — caret selectionStart on arrival', () => {
    it('places caret on first logical line at clamped column after ArrowDown move (edge=first)', async () => {
        // anchorSlice = "line1\nline2" — two logical lines.
        // pendingCaret: edge='first', column=3 → selectionStart should be 3 (on "line1").
        const twoLineContent = 'line1\nline2\n';
        const twoLinePool: PoolEntry[] = [
            { t: 0, r: [0, 12], d: 0 }, // "line1\nline2\n"
        ];
        const et = makeEditTarget(0, 12, 'line1\nline2');

        render(
            <CaretArrivalHarness
                pool={twoLinePool}
                content={twoLineContent}
                editTarget={et}
                pendingCaret={{ edge: 'first', column: 3 }}
            />,
        );

        // After mount, the layout effect should have placed the caret.
        await act(async () => {});

        const textarea = document.querySelector('textarea') as HTMLTextAreaElement;
        expect(textarea).not.toBeNull();
        // First line is "line1" (length=5). Column 3 < 5 → selectionStart=3.
        expect(textarea.selectionStart).toBe(3);
        expect(textarea.selectionEnd).toBe(3);
    });

    it('places caret on last logical line at clamped column after ArrowUp move (edge=last)', async () => {
        // anchorSlice = "first\nsecond\nthird" — three logical lines.
        // pendingCaret: edge='last', column=2 → selectionStart should be at "third"[2] = 14.
        const multiLineContent = 'first\nsecond\nthird\n';
        const multiLinePool: PoolEntry[] = [
            { t: 0, r: [0, 19], d: 0 },
        ];
        const et = makeEditTarget(0, 19, 'first\nsecond\nthird');

        render(
            <CaretArrivalHarness
                pool={multiLinePool}
                content={multiLineContent}
                editTarget={et}
                pendingCaret={{ edge: 'last', column: 2 }}
            />,
        );

        await act(async () => {});

        const textarea = document.querySelector('textarea') as HTMLTextAreaElement;
        expect(textarea).not.toBeNull();
        // Last line is "third" (length=5). Column 2 < 5 → selectionStart = 6+7+2 = 15.
        // "first\n" = 6, "second\n" = 7, so "third"[2] is at 6+7+2 = 15.
        expect(textarea.selectionStart).toBe(15);
        expect(textarea.selectionEnd).toBe(15);
    });

    it('clamps column to line length on first line', async () => {
        // anchorSlice = "hi\nworld" — first line is "hi" (length=2).
        // pendingCaret: edge='first', column=10 → clamps to 2.
        const content = 'hi\nworld\n';
        const pool: PoolEntry[] = [{ t: 0, r: [0, 9], d: 0 }];
        const et = makeEditTarget(0, 9, 'hi\nworld');

        render(
            <CaretArrivalHarness
                pool={pool}
                content={content}
                editTarget={et}
                pendingCaret={{ edge: 'first', column: 10 }}
            />,
        );

        await act(async () => {});

        const textarea = document.querySelector('textarea') as HTMLTextAreaElement;
        expect(textarea.selectionStart).toBe(2); // clamped to "hi".length
        expect(textarea.selectionEnd).toBe(2);
    });

    it('clamps column to line length on last line', async () => {
        // anchorSlice = "hello\nhi" — last line is "hi" (length=2).
        // pendingCaret: edge='last', column=99 → clamps to 2.
        const content = 'hello\nhi\n';
        const pool: PoolEntry[] = [{ t: 0, r: [0, 9], d: 0 }];
        const et = makeEditTarget(0, 9, 'hello\nhi');

        render(
            <CaretArrivalHarness
                pool={pool}
                content={content}
                editTarget={et}
                pendingCaret={{ edge: 'last', column: 99 }}
            />,
        );

        await act(async () => {});

        const textarea = document.querySelector('textarea') as HTMLTextAreaElement;
        // "hello\n" = 6 chars. "hi" clamped to 2 → selectionStart = 6 + 2 = 8.
        expect(textarea.selectionStart).toBe(8);
        expect(textarea.selectionEnd).toBe(8);
    });
});
