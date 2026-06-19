/**
 * G6+G7 settle-gate integration tests.
 *
 * Root cause: the 250ms reland backstop timer fired BEFORE the async re-render
 * reflecting the commit arrived, landing the editor on stale renderedContent.
 * This caused G7 (stale seed) and G6 (self-heal DROP from bad anchorSlice).
 *
 * Fix: `executeLanding` now gates on renderedContent identity — if
 * `preCommitContentRef.current !== null && renderedContentRef.current === preCommitContentRef.current`
 * it returns without consuming the pending landing. The props-change layout
 * effect re-fires once the settled render arrives.
 *
 * T1: settle-gate defers the 250ms timer when content hasn't advanced.
 * T2: self-heal KEEPs a freshly-landed nested editor (no DROP).
 *
 * Fixture reuses the p3-3-nesting blockquote setup:
 *   content = '> line one\n> line two\n\npara2\n\n'  (30 bytes)
 *   pool[0] BlockQuote: {t:0, r:[0,23], d:0}
 *   pool[1] ChildPara:  {t:0, r:[2,22], d:0}
 *   pool[2] Para2:      {t:0, r:[23,30], d:0}
 *   nestedEditBuffers: '0:2-22:0' -> 'line one\nline two'
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

const PLATFORM = detectPlatform();

function nestingChord(key: 'ArrowLeft' | 'ArrowRight'): KeyboardEventInit {
    return PLATFORM === 'mac'
        ? { key, metaKey: true, ctrlKey: true, altKey: false, shiftKey: false }
        : { key, altKey: true, shiftKey: true, metaKey: false, ctrlKey: false };
}

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

/* ─── Fixture ──────────────────────────────────────────────────────────────── */

const CONTENT = '> line one\n> line two\n\npara2\n\n';

const POOL = [
    { t: 0, r: [0, 23], d: 0 },   // pool[0]: BlockQuote
    { t: 0, r: [2, 22], d: 0 },   // pool[1]: ChildPara
    { t: 0, r: [23, 30], d: 0 },  // pool[2]: Para2
];

const CHILD_SI_KEY = '0:2-22:0';
const CLEAN_BUFFER = 'line one\nline two';

// The blockquote's anchorSlice (verbatim bytes [0,23]):
const BQ_ANCHOR_SLICE = '> line one\n> line two';

function makeAstJson(pool = POOL): string {
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
        astContext: { p: pool },
    };
    return JSON.stringify(ast);
}

function mountFixture(opts: {
    setAst?: (ast: PandocAST) => void;
    unlockNestingCursor?: boolean;
    nestedEditBuffers?: Record<string, string>;
    pool?: typeof POOL;
    content?: string;
} = {}) {
    const setAst = opts.setAst ?? vi.fn();
    const pool = opts.pool ?? POOL;
    const content = opts.content ?? CONTENT;
    const astJson = makeAstJson(pool);

    const props: PreviewRootProps = {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: content,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst,
        unlockNestingCursor: opts.unlockNestingCursor,
        nestedEditBuffers: opts.nestedEditBuffers,
        onNavigateToDocument: () => {},
    };

    const result = render(<PreviewRoot {...props} />);
    return { ...result, setAst };
}

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
 * T1 ★ (fail-on-revert): Settle-gate defers landing until content advances.
 *
 * Without the gate: the 250ms timer fires while renderedContent is still the
 * pre-commit value → executeLanding opens the editor with a stale anchorSlice
 * (the pre-edit blockquote text, missing the dirty edit) → T1's discriminator
 * assertion `textarea.value !== DIRTY_EDIT` → RED.
 *
 * With the gate: the timer fires but executeLanding sees
 * preCommitContentRef.current === renderedContentRef.current → returns without
 * consuming → pendingLanding still armed → the settled rerender triggers the
 * props-change layout effect → executeLanding now sees fresh content → opens
 * with the committed slice → GREEN.
 *
 * Discriminator: `textarea.value` contains the committed edit text ("edited").
 * Do NOT relax this to a presence check — without the gate the editor opens
 * on BOTH the stale timer and the settled render.
 *
 * FAIL-ON-REVERT: Remove the settle-gate `if (preCommitContentRef.current !== null
 * && renderedContentRef.current === preCommitContentRef.current) return;` from
 * executeLanding → timer lands on stale content → textarea.value === BQ_ANCHOR_SLICE
 * (pre-edit, missing "edited") → RED.
 * ─────────────────────────────────────────────────────────────────────────── */

describe('G6+G7 T1 ★ — settle-gate defers 250ms timer until content advances', () => {
    it('lands on FRESH committed slice after settled rerender, not stale pre-edit content', async () => {
        // Use fake timers so we control when the 250ms backstop fires.
        vi.useFakeTimers();

        const setAst = vi.fn();
        const { container, rerender } = mountFixture({
            setAst,
            unlockNestingCursor: true,
            nestedEditBuffers: { [CHILD_SI_KEY]: CLEAN_BUFFER },
        });

        await act(async () => {});
        mockTileRects(container);

        // Step 1: activate the inner child para (pool[1]).
        const childPara = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(childPara).not.toBeNull();
        await act(async () => {
            fireEvent(childPara!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childPara!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        let textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();
        expect(textarea!.value).toBe(CLEAN_BUFFER);

        // Step 2: dirty the draft.
        const DIRTY_EDIT = 'line one edited\nline two';
        await act(async () => {
            fireEvent.change(textarea!, { target: { value: DIRTY_EDIT } });
        });
        textarea = container.querySelector<HTMLTextAreaElement>('textarea')!;
        expect(textarea!.value).toBe(DIRTY_EDIT);

        // Step 3: nest-out (dirty path → commitAndArmReland fires).
        await act(async () => {
            fireEvent.keyDown(textarea!, nestingChord('ArrowLeft'));
        });

        // setAst was called once (the dirty commit).
        expect(setAst).toHaveBeenCalledOnce();
        // Editor is closed (commitAndArmReland closed it).
        expect(container.querySelector('textarea')).toBeNull();

        // Step 4: advance past 250ms WITHOUT a prop swap (content still pre-commit).
        // The settle-gate must prevent executeLanding from consuming the pending landing.
        await act(async () => {
            vi.advanceTimersByTime(300);
        });

        // Gate must have deferred — no textarea opened yet.
        expect(container.querySelector('textarea')).toBeNull();

        // Step 5: simulate the settled re-render with fresh content/ast reflecting the edit.
        // The committed edit changed "line one\nline two" → "line one edited\nline two" in the
        // child para buffer. The blockquote's raw bytes shift accordingly.
        // For the test, the key invariant is that renderedContent !== pre-commit content.
        const SETTLED_CONTENT = '> line one edited\n> line two\n\npara2\n\n';
        const settledPool = [
            { t: 0, r: [0, 31], d: 0 },   // BlockQuote (larger now)
            { t: 0, r: [2, 30], d: 0 },   // ChildPara (larger)
            { t: 0, r: [31, 38], d: 0 },  // Para2
        ];
        const settledAstJson = makeAstJson(settledPool);

        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={settledAstJson}
                    untransformedAstJson={settledAstJson}
                    renderedContent={SETTLED_CONTENT}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    unlockNestingCursor
                    nestedEditBuffers={{ [CHILD_SI_KEY]: CLEAN_BUFFER }}
                    onNavigateToDocument={() => {}}
                />,
            );
        });
        mockTileRects(container);

        // Step 6: reland layout effect fires on the NEW renderedContent → executeLanding
        // no longer sees preCommitContent === current → lands successfully on the
        // blockquote (nest-out destination).
        const textareaAfter = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textareaAfter).not.toBeNull();

        // DISCRIMINATOR: the landed editor's value must reflect the fresh committed slice.
        // The blockquote's anchorSlice in SETTLED_CONTENT is '> line one edited\n> line two'.
        // It must NOT be the pre-commit BQ_ANCHOR_SLICE ('> line one\n> line two').
        expect(textareaAfter!.value).toContain('edited');
        expect(textareaAfter!.value).not.toBe(BQ_ANCHOR_SLICE);
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * T2 ★ (fail-on-revert): Self-heal KEEPs a freshly-landed nested editor.
 *
 * Without the gate: the 250ms timer fires on stale content → executeLanding
 * opens the blockquote editor with a stale anchorSlice. When the settled
 * re-render arrives, the self-heal effect runs findReanchorCandidate against
 * the blockquote's stale anchorSlice. The stale anchorSlice doesn't match the
 * new content (the edit is baked in) → no candidate → DROP → textarea null → RED.
 *
 * With the gate: the editor opens only on fresh content → its anchorSlice is a
 * verbatim slice of current renderedContent → self-heal's byte-verify passes
 * (KEEP, possibly with a re-anchor) → textarea still present → GREEN.
 *
 * FAIL-ON-REVERT: Same gate hunk as T1. Without the gate, the stale-seeded
 * editor opens early and the post-land settling re-render makes self-heal DROP
 * → textarea becomes null → "textarea still present" assertion → RED.
 * ─────────────────────────────────────────────────────────────────────────── */

describe('G6+G7 T2 ★ — self-heal KEEPs the freshly-landed editor after settled rerender', () => {
    it('textarea stays open and setAst not called again after a second stable rerender', async () => {
        vi.useFakeTimers();

        const setAst = vi.fn();
        const { container, rerender } = mountFixture({
            setAst,
            unlockNestingCursor: true,
            nestedEditBuffers: { [CHILD_SI_KEY]: CLEAN_BUFFER },
        });

        await act(async () => {});
        mockTileRects(container);

        // Activate child para.
        const childPara = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(childPara).not.toBeNull();
        await act(async () => {
            fireEvent(childPara!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childPara!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        let textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();

        // Dirty the draft.
        const DIRTY_EDIT = 'line one edited\nline two';
        await act(async () => {
            fireEvent.change(textarea!, { target: { value: DIRTY_EDIT } });
        });

        // Nest-out (dirty path → commit + arm reland).
        await act(async () => {
            fireEvent.keyDown(container.querySelector<HTMLTextAreaElement>('textarea')!, nestingChord('ArrowLeft'));
        });
        expect(setAst).toHaveBeenCalledOnce();
        expect(container.querySelector('textarea')).toBeNull();

        // Advance 250ms — gate defers (stale content).
        await act(async () => {
            vi.advanceTimersByTime(300);
        });
        expect(container.querySelector('textarea')).toBeNull();

        // Settled re-render: fresh content.
        const SETTLED_CONTENT = '> line one edited\n> line two\n\npara2\n\n';
        const settledPool = [
            { t: 0, r: [0, 31], d: 0 },
            { t: 0, r: [2, 30], d: 0 },
            { t: 0, r: [31, 38], d: 0 },
        ];
        const settledAstJson = makeAstJson(settledPool);

        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={settledAstJson}
                    untransformedAstJson={settledAstJson}
                    renderedContent={SETTLED_CONTENT}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    unlockNestingCursor
                    nestedEditBuffers={{ [CHILD_SI_KEY]: CLEAN_BUFFER }}
                    onNavigateToDocument={() => {}}
                />,
            );
        });
        mockTileRects(container);

        // Editor should now be open (reland succeeded with fresh content).
        textarea = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(textarea).not.toBeNull();

        // Capture setAst call count after land.
        const callsAfterLand = setAst.mock.calls.length;
        expect(callsAfterLand).toBe(1); // still only the one commit

        // Step: fire ONE MORE stable re-render (same content, no edit).
        // The self-heal effect fires. With a fresh anchorSlice (verbatim of current
        // content), findReanchorCandidate KEEPS the editor → no DROP, no extra setAst.
        await act(async () => {
            rerender(
                <PreviewRoot
                    astJson={settledAstJson}
                    untransformedAstJson={settledAstJson}
                    renderedContent={SETTLED_CONTENT}
                    currentFilePath="/test.qmd"
                    assetManifest={{}}
                    setAst={setAst}
                    unlockNestingCursor
                    nestedEditBuffers={{ [CHILD_SI_KEY]: CLEAN_BUFFER }}
                    onNavigateToDocument={() => {}}
                />,
            );
        });

        // Textarea STILL present after the second stable re-render (self-heal KEEP).
        expect(container.querySelector('textarea')).not.toBeNull();

        // setAst NOT called again (no DROP-induced commit).
        expect(setAst).toHaveBeenCalledTimes(callsAfterLand);
    });
});
