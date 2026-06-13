/**
 * P3.3 integration tests: depth-cursor seeding, dirty guard, and nested commit.
 *
 * All five cases drive the REAL PreviewRoot (no custom registry) so real
 * BlockQuote / Para components render nested [data-block-pool-id] elements.
 *
 * Fixture
 * ───────
 * content = '> line one\n> line two\n\npara2\n\n'  (30 bytes)
 *
 * pool:
 *   pool[0] BlockQuote: {t:0, r:[0,23],  d:0}  siKey='0:0-23:0'
 *   pool[1] ChildPara:  {t:0, r:[2,22],  d:0}  siKey='0:2-22:0'
 *   pool[2] Para2:      {t:0, r:[23,30], d:0}  siKey='0:23-30:0'
 *
 * anchorSlice values (normalizeLineEndings(sliceBytes(content,r0,r1)).trimEnd()):
 *   BlockQuote -> '> line one\n> line two'  (prefixed, 2 lines)
 *   ChildPara  -> 'line one\n> line two'    (partial prefix on line 2)
 *   Para2      -> 'para2'
 *
 * nestedEditBuffers:
 *   '0:2-22:0' -> 'line one\nline two'  (CLEAN buffer: no '> ' prefix)
 *
 * DOM layout (real components, default registry):
 *   <blockquote data-block-pool-id="0">
 *     <p data-block-pool-id="1"> line one / line two </p>
 *   </blockquote>
 *   <p data-block-pool-id="2">para2</p>
 *
 * Fail-on-revert notes are embedded in each starred test.
 */

// @vitest-environment jsdom

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, act, fireEvent } from '@testing-library/react';
import React from 'react';
import { PreviewRoot } from './PreviewRoot';
import type { PreviewRootProps } from './PreviewRoot';
import type { PandocAST } from '../framework';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.useRealTimers();
});

/* ─── PointerEvent helper (same as p2-4-real) ────────────────────────────── */
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

/* ─── Fixture ────────────────────────────────────────────────────────────── */

const CONTENT = '> line one\n> line two\n\npara2\n\n';

// pool[0] BlockQuote: r=[0,23]   anchorSlice = '> line one\n> line two'
// pool[1] ChildPara:  r=[2,22]   anchorSlice = 'line one\n> line two'
// pool[2] Para2:      r=[23,30]  anchorSlice = 'para2'
const POOL = [
    { t: 0, r: [0, 23], d: 0 },   // pool[0]: BlockQuote
    { t: 0, r: [2, 22], d: 0 },   // pool[1]: ChildPara
    { t: 0, r: [23, 30], d: 0 },  // pool[2]: Para2
];

// Clean buffer for the child para (no '> ' prefix, no trailing whitespace).
const CHILD_SI_KEY = '0:2-22:0';
const CLEAN_BUFFER = 'line one\nline two';

/**
 * Build the AST JSON for the blockquote + para2 fixture.
 * untransformedAstJson === astJson so sourceIndex resolves all blocks.
 */
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
        unlockDepthCursor?: boolean;
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
        unlockDepthCursor: opts.unlockDepthCursor,
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
 * Each gets a distinct non-zero rect so isVisibleTile passes.
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
 * Test 1: Leaf resolution (unlocked)
 *
 * unlockDepthCursor=true; click the inner child Para → textarea opens for the
 * CHILD (value = clean buffer), NOT the whole blockquote.
 *
 * Production path:
 *   activate(el) → tile = el.closest('[data-block-pool-id]') (leaf, not collapse)
 *   → captureEditTarget(p[1]) → pool[1] → anchorR0=2
 *   → siKey='0:2-22:0' → seededDraft = nestedEditBuffers['0:2-22:0'] = CLEAN_BUFFER
 *   → editDraftRef.current = CLEAN_BUFFER
 *   → textarea value = CLEAN_BUFFER
 * ─────────────────────────────────────────────────────────────────────────── */

describe('P3.3 test 1 — leaf resolution (unlocked): click child Para opens child editor', () => {
    it('opens the child para editor (not blockquote) with clean buffer value', async () => {
        const setAst = vi.fn();
        const { container } = mountFixture({
            setAst,
            unlockDepthCursor: true,
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

        const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();

        // Must open for the CHILD (value = clean buffer, no '> ').
        // If leaf resolution were absent (still using resolveLockedTile), the
        // blockquote would be selected and value = '> line one\n> line two'.
        expect(textarea!.value).toBe(CLEAN_BUFFER);
        expect(textarea!.value).not.toContain('> ');
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 2: Locked contrast
 *
 * unlockDepthCursor omitted (default locked); click the inner child Para →
 * textarea opens for the whole BLOCKQUOTE (value includes '> ').
 *
 * This proves the mode branch. If leaf resolution were unconditional this fails.
 * ─────────────────────────────────────────────────────────────────────────── */

describe('P3.3 test 2 — locked contrast: click child Para opens blockquote editor', () => {
    it('opens the blockquote editor (not child) in locked (default) mode', async () => {
        const setAst = vi.fn();
        const { container } = mountFixture({ setAst });

        await act(async () => {});
        mockTileRects(container);

        // Click the inner child para.
        const childPara = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(childPara).not.toBeNull();

        await act(async () => {
            fireEvent(childPara!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childPara!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();

        // Must open for the BLOCKQUOTE (contains '> ').
        // resolveLockedTile climbs to the blockquote (PREFIXING_TAGS).
        expect(textarea!.value).toContain('> ');
        // Specifically: the blockquote's anchorSlice.
        expect(textarea!.value).toBe('> line one\n> line two');
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 3 ★ (fail-on-revert): Buffer-seed shows clean draft + dirty guard
 *
 * unlocked; click child → textarea value === CLEAN_BUFFER (no '> ').
 * Then blur WITHOUT typing → setAst NOT called (baseline = seededDraft).
 *
 * FAIL-ON-REVERT: if the baseline is changed back to `anchorSlice` (= the
 * polluted 'line one\n> line two'), the seeded clean draft !== anchorSlice →
 * the dirty guard incorrectly sees it as "dirty" → blur commits (setAst
 * called once) → this assertion `expect(setAst).not.toHaveBeenCalled()` FAILS.
 *
 * Fail-on-revert red line (confirmed):
 *   AssertionError: expected "spy" to not have been called at all, but actually been called 1 times
 * ─────────────────────────────────────────────────────────────────────────── */

describe('P3.3 test 3 ★ — buffer-seed: clean draft + dirty guard baseline', () => {
    it('seeds clean buffer, and blur without edit does NOT commit (seededDraft baseline)', async () => {
        const setAst = vi.fn();
        const { container } = mountFixture({
            setAst,
            unlockDepthCursor: true,
            nestedEditBuffers: { [CHILD_SI_KEY]: CLEAN_BUFFER },
        });

        await act(async () => {});
        mockTileRects(container);

        // Click the child para to open its editor.
        const childPara = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(childPara).not.toBeNull();

        await act(async () => {
            fireEvent(childPara!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childPara!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();

        // Textarea seeded with clean buffer (no '> ').
        expect(textarea!.value).toBe(CLEAN_BUFFER);

        // Blur WITHOUT any typing → should NOT commit.
        // If dirty guard baselines on anchorSlice ('line one\n> line two')
        // instead of seededDraft (CLEAN_BUFFER), then:
        //   normalized('line one\nline two') !== 'line one\n> line two' → commits (BUG).
        // With correct seededDraft baseline:
        //   normalized(CLEAN_BUFFER) === CLEAN_BUFFER → no commit.
        await act(async () => {
            fireEvent.blur(textarea!);
        });

        // MUST NOT have committed.
        expect(setAst).not.toHaveBeenCalled();
        // Editor closes (no commit → setEditTarget(null) on the cancel path).
        expect(container.querySelector('textarea')).toBeNull();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 4 ★ (fail-on-revert): Commit destination from LIVE identity (Cmd-Enter)
 *
 * unlocked; click child, type a change, Cmd-Enter →
 * setAst called once with:
 *   payload.destinationSourceInfoJson = JSON.stringify({t:0, r:[2,22], d:0})
 *   payload.channel === 'text'
 *
 * FAIL-ON-REVERT: if commitDepthEdit is made a no-op (early return), setAst
 * is not called → `expect(setAst).toHaveBeenCalledOnce()` FAILS.
 *
 * Fail-on-revert red line (confirmed):
 *   AssertionError: expected "spy" to have been called once, but got 0 times
 * ─────────────────────────────────────────────────────────────────────────── */

describe('P3.3 test 4 ★ — Cmd-Enter commit via LIVE identity (commitDepthEdit)', () => {
    it('commits to the child para destination with live anchorR0/R1 on Cmd-Enter', async () => {
        const setAst = vi.fn();
        const { container } = mountFixture({
            setAst,
            unlockDepthCursor: true,
            nestedEditBuffers: { [CHILD_SI_KEY]: CLEAN_BUFFER },
        });

        await act(async () => {});
        mockTileRects(container);

        // Open the child para editor.
        const childPara = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(childPara).not.toBeNull();

        await act(async () => {
            fireEvent(childPara!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childPara!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe(CLEAN_BUFFER);

        // Type a change to make the draft dirty.
        await act(async () => {
            fireEvent.change(textarea!, { target: { value: 'line one edited\nline two' } });
        });

        const ta = container.querySelector<HTMLTextAreaElement>('textarea')!;

        // Cmd-Enter → commitDepthEdit → setAst called with live identity.
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'Enter', metaKey: true });
        });

        // Must have committed exactly once.
        expect(setAst).toHaveBeenCalledOnce();

        const payload = setAst.mock.calls[0][0] as any;
        expect(payload.__isPreviewNodeEdit).toBe(true);
        expect(payload.channel).toBe('text');

        // Destination must be the LIVE child para identity: {t:0, r:[2,22], d:0}.
        const dest = JSON.parse(payload.destinationSourceInfoJson);
        expect(dest).toEqual({ t: 0, r: [2, 22], d: 0 });

        // Editor closed.
        expect(container.querySelector('textarea')).toBeNull();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 5: siKey-shift preserves draft
 *
 * Proves that editDraftRef preserves the in-flight draft when a self-heal
 * re-anchor occurs (the edited block shifts in the pool but its CONTENT is
 * unchanged). The draft must NOT be reset from the now-missing nestedEditBuffers
 * entry or from anchorSlice.
 *
 * Fixture: 3 plain Para blocks (no nested blockquote needed for this test).
 *   content = 'short\npara2\npara3\n\n'  (20 bytes)
 *   pool[0] short:   {t:0, r:[0,6],  d:0}  anchorSlice='short'
 *   pool[1] para2:   {t:0, r:[6,12], d:0}  anchorSlice='para2'  ← EDIT TARGET
 *   pool[2] para3:   {t:0, r:[12,20],d:0}  anchorSlice='para3'
 *
 * siKey for para2 = '0:6-12:0'.
 * nestedEditBuffers['0:6-12:0'] = 'buffer text'  (simulated clean buffer).
 *
 * After open + type 'edited text':
 *   Rerender where 'short' grows to 'short extended\n' (15 bytes), shifting para2.
 *   New pool:
 *     pool[0] short_ext: {t:0, r:[0,15],  d:0}  anchorSlice='short extended'
 *     pool[1] para2_sh:  {t:0, r:[15,21], d:0}  anchorSlice='para2'  ← KEEP
 *     pool[2] para3_sh:  {t:0, r:[21,29], d:0}  anchorSlice='para3'
 *   New siKey for para2 = '0:15-21:0' (old '0:6-12:0' is gone from new pool).
 *   New nestedEditBuffers does NOT contain '0:6-12:0'.
 *
 * Self-heal: findReanchorCandidate(pool, newContent, anchorR0=6, anchorSlice='para2')
 *   nearest at/after r0=6 = pool[1] r0=15.
 *   content-verify: newContent.slice(15,21).trimEnd() = 'para2' === 'para2' ✓ → KEEP.
 *   (No competing Original block exists between r0=6 and r0=15.)
 *
 * The self-heal keeps the editor open (re-anchors to r0=15). The textarea
 * still shows 'edited text' because editDraftRef.current was set by onChange
 * and is not reset during re-anchor (setEditTargetRaw preserves the draft ref).
 * ─────────────────────────────────────────────────────────────────────────── */

describe('P3.3 test 5 — siKey-shift preserves in-flight draft', () => {
    it('draft survives self-heal re-anchor when siKey shifts (block above grows)', async () => {
        // 3-Para fixture (no blockquote — avoids competing pool entries during shift).
        const content5 = 'short\npara2\npara3\n\n';
        const pool5 = [
            { t: 0, r: [0, 6], d: 0 },    // pool[0]: 'short\n'
            { t: 0, r: [6, 12], d: 0 },   // pool[1]: 'para2\n'  ← edit target
            { t: 0, r: [12, 20], d: 0 },  // pool[2]: 'para3\n\n'
        ];
        const siKey5 = '0:6-12:0';
        const buffer5 = 'buffer text'; // simulated clean buffer for para2

        const ast5 = {
            'pandoc-api-version': [1, 23, 0],
            meta: {},
            blocks: [
                { t: 'Para', c: [{ t: 'Str', c: 'short' }], s: 0 },
                { t: 'Para', c: [{ t: 'Str', c: 'para2' }], s: 1 },
                { t: 'Para', c: [{ t: 'Str', c: 'para3' }], s: 2 },
            ],
            astContext: { p: pool5 },
        };
        const astJson5 = JSON.stringify(ast5);

        const setAst = vi.fn();
        const props: PreviewRootProps = {
            astJson: astJson5,
            untransformedAstJson: astJson5,
            renderedContent: content5,
            currentFilePath: '/test.qmd',
            assetManifest: {},
            setAst,
            unlockDepthCursor: true,
            nestedEditBuffers: { [siKey5]: buffer5 },
            onNavigateToDocument: () => {},
        };

        const { container, rerender } = render(<PreviewRoot {...props} />);
        await act(async () => {});
        mockTileRects(container);

        // Open the para2 editor (pool[1], data-block-pool-id="1").
        const tile1 = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(tile1).not.toBeNull();

        await act(async () => {
            fireEvent(tile1!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tile1!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        // Seeded with the clean buffer (from nestedEditBuffers[siKey5]).
        expect(textarea!.value).toBe(buffer5);

        // Type an edit.
        await act(async () => {
            fireEvent.change(textarea!, { target: { value: 'edited text' } });
        });

        const taAfterEdit = container.querySelector<HTMLTextAreaElement>('textarea')!;
        expect(taAfterEdit.value).toBe('edited text');

        // Simulate re-render: 'short\n' (6 bytes) grows to 'short extended\n' (15 bytes).
        // para2's content is UNCHANGED ('para2\n') but its offset shifts from 6 to 15.
        const newContent5 = 'short extended\npara2\npara3\n\n';
        const newPool5 = [
            { t: 0, r: [0, 15], d: 0 },   // pool[0]: 'short extended\n'
            { t: 0, r: [15, 21], d: 0 },  // pool[1]: 'para2\n' shifted
            { t: 0, r: [21, 29], d: 0 },  // pool[2]: 'para3\n\n' shifted
        ];

        const newAst5 = {
            'pandoc-api-version': [1, 23, 0],
            meta: {},
            blocks: [
                { t: 'Para', c: [{ t: 'Str', c: 'short extended' }], s: 0 },
                { t: 'Para', c: [{ t: 'Str', c: 'para2' }], s: 1 },
                { t: 'Para', c: [{ t: 'Str', c: 'para3' }], s: 2 },
            ],
            astContext: { p: newPool5 },
        };
        const newAstJson5 = JSON.stringify(newAst5);

        // New nestedEditBuffers does NOT contain the old siKey '0:6-12:0'.
        // (New siKey would be '0:15-21:0' but we leave it absent to test that
        // the draft is NOT re-derived from missing buffer.)
        const newNestedBuffers5: Record<string, string> = {};

        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={newAstJson5}
                    untransformedAstJson={newAstJson5}
                    renderedContent={newContent5}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    unlockDepthCursor={true}
                    nestedEditBuffers={newNestedBuffers5}
                    onNavigateToDocument={() => {}}
                />,
            );
        });

        mockTileRects(container);

        // The textarea should still be open (self-heal KEEP: para2 content unchanged).
        const taAfterRerender = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(taAfterRerender).not.toBeNull();

        // Draft MUST be preserved — NOT reset to the (absent) new buffer or anchorSlice.
        // editDraftRef.current was set to 'edited text' by onChange and survives the re-anchor.
        expect(taAfterRerender!.value).toBe('edited text');
    });
});
