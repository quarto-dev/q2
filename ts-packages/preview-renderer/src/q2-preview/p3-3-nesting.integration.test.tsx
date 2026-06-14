/**
 * P3.3 §3b integration tests: nesting-KEY navigation (ArrowLeft/ArrowRight with
 * platform chord to move the nesting cursor out/in along the AST path).
 *
 * All six cases drive the REAL PreviewRoot (no custom registry) so real
 * BlockQuote / Para components render nested [data-block-pool-id] elements.
 *
 * Fixture (same as p3-3-seeding):
 * ───────
 * content = '> line one\n> line two\n\npara2\n\n'  (30 bytes)
 *
 * pool:
 *   pool[0] BlockQuote: {t:0, r:[0,23],  d:0}  siKey='0:0-23:0'
 *   pool[1] ChildPara:  {t:0, r:[2,22],  d:0}  siKey='0:2-22:0'
 *   pool[2] Para2:      {t:0, r:[23,30], d:0}  siKey='0:23-30:0'
 *
 * DOM layout (real components, default registry):
 *   <blockquote data-block-pool-id="0">
 *     <p data-block-pool-id="1"> line one / line two </p>
 *   </blockquote>
 *   <p data-block-pool-id="2">para2</p>
 *
 * nestedEditBuffers:
 *   '0:2-22:0' -> 'line one\nline two'  (CLEAN buffer: no '> ' prefix)
 *
 * Nesting navigation semantics:
 *   'out' (ArrowLeft chord) = move to AST parent; clamp at outermost.
 *   'in'  (ArrowRight chord) = move toward leafAnchorR0; clamp at leaf.
 *   Nesting moves re-seed the draft from the new node's buffer/anchorSlice.
 *   Nesting moves do NOT commit (setAst is NOT called).
 *   leafAnchorR0 is preserved across nesting moves.
 */

// @vitest-environment jsdom

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, act, fireEvent } from '@testing-library/react';
import React from 'react';
import { PreviewRoot } from './PreviewRoot';
import type { PreviewRootProps } from './PreviewRoot';
import type { PandocAST } from '../framework';
import { detectPlatform } from './nestingNav';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.useRealTimers();
});

/* ─── Platform detection ─────────────────────────────────────────────────────
 * Compute the correct modifier chord at import time so the same test file
 * works on macOS (Cmd+Ctrl) and Linux/Windows (Alt+Shift).
 */
const PLATFORM = detectPlatform();

/** Build a keydown event object for the nesting chord + the given arrow key. */
function nestingChord(key: 'ArrowLeft' | 'ArrowRight'): KeyboardEventInit {
    return PLATFORM === 'mac'
        ? { key, metaKey: true, ctrlKey: true, altKey: false, shiftKey: false }
        : { key, altKey: true, shiftKey: true, metaKey: false, ctrlKey: false };
}
// 'out' = ArrowLeft, 'in' = ArrowRight

/* ─── PointerEvent helper (same as p3-3-seeding) ────────────────────────────── */
function ptrEvent(
    type: string,
    opts: PointerEventInit & { clientX?: number; clientY?: number } = {},
): Event {
    const PE = (window as any).PointerEvent ?? Event;
    const evt = new PE(type, { bubbles: true, cancelable: true, ...opts });
    for (const [key, val] of Object.entries({
        ...(opts.pointerType !== undefined ? { pointerType: opts.pointerType } : {}),
        ...(opts.clientX !== undefined ? { clientX: opts.clientX } : {}),
        ...(opts.clientY !== undefined ? { clientY: opts.clientY } : {}),
    } as Record<string, unknown>)) {
        Object.defineProperty(evt, key, { value: val, configurable: true });
    }
    return evt;
}

/* ─── Fixture ────────────────────────────────────────────────────────────────── */

const CONTENT = '> line one\n> line two\n\npara2\n\n';

// pool[0] BlockQuote: r=[0,23]   anchorSlice = '> line one\n> line two'
// pool[1] ChildPara:  r=[2,22]   anchorSlice = 'line one\n> line two'
// pool[2] Para2:      r=[23,30]  anchorSlice = 'para2'
const POOL = [
    { t: 0, r: [0, 23], d: 0 },   // pool[0]: BlockQuote
    { t: 0, r: [2, 22], d: 0 },   // pool[1]: ChildPara
    { t: 0, r: [23, 30], d: 0 },  // pool[2]: Para2
];

// Clean buffer for the child para (no '> ' prefix).
const CHILD_SI_KEY = '0:2-22:0';
const CLEAN_BUFFER = 'line one\nline two';

// anchorSlice for the blockquote (prefixed, 2 lines)
const BQ_ANCHOR_SLICE = '> line one\n> line two';

function makeAstJson(): string {
    const ast = {
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks: [
            {
                t: 'BlockQuote',
                c: [
                    {
                        t: 'Para',
                        c: [
                            { t: 'Str', c: 'line one' },
                            { t: 'SoftBreak' },
                            { t: 'Str', c: 'line two' },
                        ],
                        s: 1,
                    },
                ],
                s: 0,
            },
            {
                t: 'Para',
                c: [{ t: 'Str', c: 'para2' }],
                s: 2,
            },
        ],
        astContext: { p: POOL },
    };
    return JSON.stringify(ast);
}

/** Mount PreviewRoot with the blockquote fixture. */
function mountFixture(
    opts: {
        setAst?: (ast: PandocAST) => void;
        unlockNestingCursor?: boolean;
        nestedEditBuffers?: Record<string, string>;
    } = {},
) {
    const setAst = opts.setAst ?? vi.fn();
    const astJson = makeAstJson();

    const props: PreviewRootProps = {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: CONTENT,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst,
        unlockNestingCursor: opts.unlockNestingCursor,
        nestedEditBuffers: opts.nestedEditBuffers,
        onNavigateToDocument: () => {},
    };

    // Do NOT pass customRegistry — use the real default registry so BlockQuote
    // and Para render real nested [data-block-pool-id] elements.
    const result = render(<PreviewRoot {...props} />);
    return { ...result, setAst };
}

/**
 * Mock getBoundingClientRect on all [data-block-pool-id] tiles.
 * Each gets a distinct non-zero rect so isVisibleBlock passes.
 */
function mockTileRects(container: HTMLElement) {
    const tiles = container.querySelectorAll<HTMLElement>('[data-block-pool-id]');
    tiles.forEach((tile) => {
        const pid = Number(tile.getAttribute('data-block-pool-id'));
        vi.spyOn(tile, 'getBoundingClientRect').mockReturnValue({
            left: 0, top: pid * 80, right: 300, bottom: pid * 80 + 60,
            width: 300, height: 60, x: 0, y: pid * 80, toJSON: () => ({}),
        } as DOMRect);
    });
}

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 1 ★ (fail-on-revert): Nesting OUT
 *
 * unlocked; click child (data-block-pool-id="1") → editor opens with CLEAN_BUFFER.
 * Fire nestingChord('ArrowLeft') → editor re-targets to BLOCKQUOTE.
 * textarea value === '> line one\n> line two'.
 * setAst NOT called (nesting move does not commit).
 *
 * FAIL-ON-REVERT: neutralize the nesting branch in onKeyDown (or make
 * requestNestingMove a no-op) → editor stays on the child / value unchanged → test fails.
 * ─────────────────────────────────────────────────────────────────────────── */

describe('P3.3 §3b test 1 ★ — nesting OUT: chord moves editor from child to blockquote', () => {
    it('re-targets to the blockquote and re-seeds draft; no commit', async () => {
        const setAst = vi.fn();
        const { container } = mountFixture({
            setAst,
            unlockNestingCursor: true,
            nestedEditBuffers: { [CHILD_SI_KEY]: CLEAN_BUFFER },
        });

        await act(async () => {});
        mockTileRects(container);

        // Click the inner child para (data-block-pool-id="1").
        const childPara = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(childPara).not.toBeNull();

        await act(async () => {
            fireEvent(childPara!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childPara!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        let textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        // Initially opens on the child with the clean buffer.
        expect(textarea!.value).toBe(CLEAN_BUFFER);

        // Fire the nesting-out chord.
        await act(async () => {
            fireEvent.keyDown(textarea!, nestingChord('ArrowLeft'));
        });

        // Re-query after re-render.
        mockTileRects(container);
        textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();

        // Editor must now target the BLOCKQUOTE (anchorSlice = '> line one\n> line two').
        // The blockquote has no clean buffer in nestedEditBuffers → seededDraft falls back to anchorSlice.
        expect(textarea!.value).toBe(BQ_ANCHOR_SLICE);

        // Nesting move must NOT commit.
        expect(setAst).not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 2: Nesting IN (from blockquote back to child)
 *
 * Start on the child → OUT to the blockquote → IN back to the child.
 * After IN: editor targets the child again; value === CLEAN_BUFFER.
 * leafAnchorR0 is preserved across moves so 'in' descends back to the child.
 * ─────────────────────────────────────────────────────────────────────────── */

describe('P3.3 §3b test 2 — nesting IN: chord moves editor from blockquote back to child', () => {
    it('re-targets to the child and re-seeds with clean buffer after out+in', async () => {
        const setAst = vi.fn();
        const { container } = mountFixture({
            setAst,
            unlockNestingCursor: true,
            nestedEditBuffers: { [CHILD_SI_KEY]: CLEAN_BUFFER },
        });

        await act(async () => {});
        mockTileRects(container);

        // Open child editor.
        const childPara = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(childPara).not.toBeNull();

        await act(async () => {
            fireEvent(childPara!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childPara!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        let textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe(CLEAN_BUFFER);

        // Fire nesting-out (child → blockquote).
        await act(async () => {
            fireEvent.keyDown(textarea!, nestingChord('ArrowLeft'));
        });
        mockTileRects(container);
        textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe(BQ_ANCHOR_SLICE);

        // Fire nesting-in (blockquote → child).
        await act(async () => {
            fireEvent.keyDown(textarea!, nestingChord('ArrowRight'));
        });
        mockTileRects(container);
        textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();

        // Must be back on the child with the clean buffer.
        expect(textarea!.value).toBe(CLEAN_BUFFER);

        // Still no commit.
        expect(setAst).not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 3: Clamp at leaf (in)
 *
 * unlocked; open child; fire nestingChord('ArrowRight') → no-op.
 * Editor stays on the child; value unchanged; setAst not called.
 * (The child has no surface strictly contained within it in the sourceIndex.)
 * ─────────────────────────────────────────────────────────────────────────── */

describe('P3.3 §3b test 3 — clamp at leaf: nesting-in on a leaf is a no-op', () => {
    it('does not move when cursor is already a leaf node', async () => {
        const setAst = vi.fn();
        const { container } = mountFixture({
            setAst,
            unlockNestingCursor: true,
            nestedEditBuffers: { [CHILD_SI_KEY]: CLEAN_BUFFER },
        });

        await act(async () => {});
        mockTileRects(container);

        // Open child editor.
        const childPara = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(childPara).not.toBeNull();

        await act(async () => {
            fireEvent(childPara!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childPara!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        let textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe(CLEAN_BUFFER);

        // Fire nesting-in on the child (which is a leaf).
        await act(async () => {
            fireEvent.keyDown(textarea!, nestingChord('ArrowRight'));
        });
        mockTileRects(container);

        // Re-query; editor must still be on the child.
        textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe(CLEAN_BUFFER);

        expect(setAst).not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 4: Clamp at outermost (out)
 *
 * Open child → OUT to blockquote → fire nestingChord('ArrowLeft') again → no-op.
 * Editor stays on the blockquote (no container above it).
 * ─────────────────────────────────────────────────────────────────────────── */

describe('P3.3 §3b test 4 — clamp at outermost: nesting-out on outermost is a no-op', () => {
    it('does not move when cursor is already at the outermost node', async () => {
        const setAst = vi.fn();
        const { container } = mountFixture({
            setAst,
            unlockNestingCursor: true,
            nestedEditBuffers: { [CHILD_SI_KEY]: CLEAN_BUFFER },
        });

        await act(async () => {});
        mockTileRects(container);

        // Open child editor.
        const childPara = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(childPara).not.toBeNull();

        await act(async () => {
            fireEvent(childPara!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childPara!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        let textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();

        // OUT to blockquote.
        await act(async () => {
            fireEvent.keyDown(textarea!, nestingChord('ArrowLeft'));
        });
        mockTileRects(container);
        textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe(BQ_ANCHOR_SLICE);

        // Fire nesting-out again (blockquote is the outermost).
        await act(async () => {
            fireEvent.keyDown(textarea!, nestingChord('ArrowLeft'));
        });
        mockTileRects(container);

        // Editor must stay on the blockquote.
        textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe(BQ_ANCHOR_SLICE);

        expect(setAst).not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 5: Bare arrow does NOT move the nesting cursor
 *
 * unlocked; open child; fire keyDown({key:'ArrowLeft'}) (NO modifiers).
 * Editor stays on the child (no re-target). The bare arrow must fall through
 * to native textarea behaviour — NOT intercepted by the nesting handler.
 * ─────────────────────────────────────────────────────────────────────────── */

describe('P3.3 §3b test 5 — bare arrow: only the chord triggers nesting movement, not bare arrows', () => {
    it('does not move when ArrowLeft is fired without the nesting modifier chord', async () => {
        const setAst = vi.fn();
        const { container } = mountFixture({
            setAst,
            unlockNestingCursor: true,
            nestedEditBuffers: { [CHILD_SI_KEY]: CLEAN_BUFFER },
        });

        await act(async () => {});
        mockTileRects(container);

        // Open child editor.
        const childPara = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(childPara).not.toBeNull();

        await act(async () => {
            fireEvent(childPara!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childPara!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        let textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe(CLEAN_BUFFER);

        // Fire a bare ArrowLeft (no modifiers).
        await act(async () => {
            fireEvent.keyDown(textarea!, {
                key: 'ArrowLeft',
                metaKey: false, ctrlKey: false, altKey: false, shiftKey: false,
            });
        });
        mockTileRects(container);

        // Editor must remain on the child (value unchanged).
        textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe(CLEAN_BUFFER);

        expect(setAst).not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 6 ★ (fail-on-revert): off ⇒ inert
 *
 * unlockNestingCursor OMITTED (locked); click the child → opens the BLOCKQUOTE
 * (locked resolution climbs via PREFIXING_TAGS). Fire nestingChord('ArrowLeft')
 * → NOTHING happens: editor stays on the blockquote; value unchanged.
 * requestNestingMove is gated behind ctx.unlockNestingCursor in onKeyDown.
 *
 * FAIL-ON-REVERT: remove the `ctx.unlockNestingCursor &&` gate in onKeyDown
 * (so locked nesting chords wrongly move) → editor moves to a parent (or throws)
 * → value changes / editor disappears → test fails.
 * ─────────────────────────────────────────────────────────────────────────── */

describe('P3.3 §3b test 6 ★ — off ⇒ inert: nesting chord is a no-op when locked', () => {
    it('nesting chord does nothing when unlockNestingCursor is not set', async () => {
        const setAst = vi.fn();
        // Mount WITHOUT unlockNestingCursor — locked mode.
        const { container } = mountFixture({
            setAst,
            nestedEditBuffers: { [CHILD_SI_KEY]: CLEAN_BUFFER },
        });

        await act(async () => {});
        mockTileRects(container);

        // In locked mode, clicking the child Para opens the BLOCKQUOTE
        // (resolveOuterBlock climbs to the blockquote via PREFIXING_TAGS).
        const childPara = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(childPara).not.toBeNull();

        await act(async () => {
            fireEvent(childPara!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childPara!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        let textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        // Locked mode: editor opens on the blockquote (value contains '> ').
        expect(textarea!.value).toBe(BQ_ANCHOR_SLICE);

        // Fire nesting-IN chord. In locked mode this must be inert (editor stays on
        // the blockquote). Without the unlockNestingCursor gate, nesting-IN would
        // descend into the child para (r=[2,22] is a direct child of blockquote),
        // changing the textarea value — that's the revert signal.
        await act(async () => {
            fireEvent.keyDown(textarea!, nestingChord('ArrowRight'));
        });
        mockTileRects(container);

        // Editor must remain on the blockquote — nesting chord is inert in locked mode.
        textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe(BQ_ANCHOR_SLICE);

        expect(setAst).not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * §1 geometry snapshot — a nest move sizes its destination from the geometry
 * CAPTURED at activation, not from the edit-distorted live DOM.
 *
 * Drives the real activate → captureGeometry → editGeometryRef → applyNestingRetarget
 * → openEditTarget(box:'snapshot') path. The editor's height is read from the
 * textarea's inline `style.height` (dispatchers sizes the textarea from
 * editTarget.contentHeight). jsdom can't lay out, so per-element rects are mocked;
 * the destination's selection is the logic under test, the rect is just the
 * environmental geometry the snapshot records.
 *
 * Discriminator: capture the blockquote at 120px and the child at 40px, then
 * POISON every live rect to a 7px sentinel before the nest move. 120px can then
 * ONLY come from the snapshot — a live measure or the keep-fallback would yield
 * 7px (or the child's 40px). Fail-on-revert (verified cold): revert
 * applyNestingRetarget's `box:'snapshot'` to the old measure-or-keep → the editor
 * sizes to the poisoned 7px live DOM → assertion RED.
 * ─────────────────────────────────────────────────────────────────────────── */

/** Mock getBoundingClientRect with a per-pid height (top=0, so height === bottom). */
function mockTileHeights(container: HTMLElement, heightByPid: Record<number, number>) {
    container.querySelectorAll<HTMLElement>('[data-block-pool-id]').forEach((tile) => {
        const pid = Number(tile.getAttribute('data-block-pool-id'));
        const h = heightByPid[pid] ?? 7;
        vi.spyOn(tile, 'getBoundingClientRect').mockReturnValue({
            left: 0, top: 0, right: 300, bottom: h,
            width: 300, height: h, x: 0, y: 0, toJSON: () => ({}),
        } as DOMRect);
    });
}

describe('§1 geometry snapshot — nest move sizes from the captured geometry, not the live DOM', () => {
    it('nest-out sizes the editor from the captured blockquote height (120px), not the poisoned live DOM', async () => {
        const setAst = vi.fn();
        const { container } = mountFixture({
            setAst,
            unlockNestingCursor: true,
            nestedEditBuffers: { [CHILD_SI_KEY]: CLEAN_BUFFER },
        });

        await act(async () => {});
        // Capture-time geometry: blockquote tall (120), child short (40).
        mockTileHeights(container, { 0: 120, 1: 40, 2: 30 });

        // Open the child → activate captures the BQ subtree snapshot
        // (BQ '0:23'→120, child '2:22'→40), BEFORE the swap to a textarea.
        const childPara = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        await act(async () => {
            fireEvent(childPara!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childPara!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });
        let textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea!.value).toBe(CLEAN_BUFFER);
        expect(textarea!.style.height).toBe('40px'); // opened on the child (live measure)

        // Poison every live rect to a 7px sentinel: now 120px can ONLY come from the snapshot.
        mockTileHeights(container, { 0: 7, 1: 7, 2: 7 });

        // Nest-out → applyNestingRetarget → openEditTarget(box:'snapshot') → BQ '0:23' → 120.
        await act(async () => {
            fireEvent.keyDown(textarea!, nestingChord('ArrowLeft'));
        });
        textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea!.value).toBe(BQ_ANCHOR_SLICE);   // moved to the blockquote
        expect(textarea!.style.height).toBe('120px');     // ← from the snapshot, not the 7px live DOM
        expect(setAst).not.toHaveBeenCalled();
    });

    it('self-heal (external re-render) CLEARS the snapshot → the next nest move falls back to the live measure', async () => {
        const setAst = vi.fn();
        const { container, rerender } = mountFixture({
            setAst,
            unlockNestingCursor: true,
            nestedEditBuffers: { [CHILD_SI_KEY]: CLEAN_BUFFER },
        });

        await act(async () => {});
        mockTileHeights(container, { 0: 120, 1: 40, 2: 30 });

        // Open the child → capture (BQ '0:23'→120).
        const childPara = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        await act(async () => {
            fireEvent(childPara!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childPara!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });
        expect(container.querySelector('textarea')!.value).toBe(CLEAN_BUFFER);

        // External re-render (e.g. a collaborator appended a paragraph): the active
        // child block [2,22] is unchanged → self-heal KEEPs it open AND clears the
        // geometry snapshot. astJson/content change → the self-heal layout effect fires.
        const newContent = CONTENT + 'para3\n\n';
        const newAst = {
            'pandoc-api-version': [1, 23, 0],
            meta: {},
            blocks: [
                { t: 'BlockQuote', c: [{ t: 'Para', c: [{ t: 'Str', c: 'line one' }, { t: 'SoftBreak' }, { t: 'Str', c: 'line two' }], s: 1 }], s: 0 },
                { t: 'Para', c: [{ t: 'Str', c: 'para2' }], s: 2 },
                { t: 'Para', c: [{ t: 'Str', c: 'para3' }], s: 3 },
            ],
            astContext: { p: [...POOL, { t: 0, r: [30, 37], d: 0 }] },
        };
        const newAstJson = JSON.stringify(newAst);
        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={newAstJson}
                    untransformedAstJson={newAstJson}
                    renderedContent={newContent}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    unlockNestingCursor
                    nestedEditBuffers={{ [CHILD_SI_KEY]: CLEAN_BUFFER }}
                    onNavigateToDocument={() => {}}
                />,
            );
        });
        // Editor stayed open on the child (self-heal KEEP).
        let textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();

        // Live geometry NOW reports the blockquote at 88px (≠ the captured 120, ≠ 7).
        mockTileHeights(container, { 0: 88, 1: 40, 2: 30, 3: 20 });

        // Nest-out → snapshot was cleared by self-heal → consume MISSES → live fallback (88px).
        await act(async () => {
            fireEvent.keyDown(textarea!, nestingChord('ArrowLeft'));
        });
        textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea!.value).toBe(BQ_ANCHOR_SLICE);
        // 88px (live fallback), NOT 120px (the stale, cleared snapshot).
        expect(textarea!.style.height).toBe('88px');
    });
});
