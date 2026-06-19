/**
 * §2 caret-aware nest-in integration tests (real PreviewRoot, jsdom).
 *
 * These prove WHICH surface a nest move targets and WHERE the caret lands —
 * pixel geometry is out of scope (jsdom has no layout); the Playwright
 * acceptance (`hub-client/e2e/q2-preview-nesting-caret-in.spec.ts`) covers the
 * real-browser caret-on-soft-wrap case.
 *
 * Fixture — a blockquote with TWO child paragraphs on different source lines, so
 * caret-toward descent differs from the frozen `leafAnchorR0` descent:
 *
 *   content = '> alpha\n>\n> gamma\n'   (18 bytes)
 *     line 0: "> alpha"   bytes 0–6   (\n @7)
 *     line 1: ">"         byte  8     (\n @9)
 *     line 2: "> gamma"   bytes 10–16 (\n @17)
 *
 *   pool[0] BlockQuote {t:0, r:[0,18], d:0}   siKey '0:0-18:0'  (verbatim, no buffer)
 *   pool[1] Para1=alpha {t:0, r:[2,7],  d:0}  siKey '0:2-7:0'   clean buffer "alpha"
 *   pool[2] Para2=gamma {t:0, r:[12,17],d:0}  siKey '0:12-17:0' clean buffer "gamma"
 *
 *   Direct children of [0,18]: Para1 (span [0,0]), Para2 (span [2,2]).
 *   childSurfaceTowardLine([0,18], Ls=2) → Para2 ; childSurfaceToward(…, leaf=2) → Para1.
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

function ptrEvent(type: string, opts: PointerEventInit = {}): Event {
    const PE = (window as unknown as { PointerEvent?: typeof PointerEvent }).PointerEvent ?? Event;
    const evt = new PE(type, { bubbles: true, cancelable: true, ...opts });
    if (opts.pointerType !== undefined) {
        Object.defineProperty(evt, 'pointerType', { value: opts.pointerType, configurable: true });
    }
    return evt;
}

const CONTENT = '> alpha\n>\n> gamma\n';

const POOL = [
    { t: 0, r: [0, 18], d: 0 },   // BlockQuote
    { t: 0, r: [2, 7], d: 0 },    // Para1 "alpha"
    { t: 0, r: [12, 17], d: 0 },  // Para2 "gamma"
];

const BUFFERS = { '0:2-7:0': 'alpha', '0:12-17:0': 'gamma' };
const BQ_SLICE = '> alpha\n>\n> gamma';

function makeAstJson(): string {
    const ast = {
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks: [
            {
                t: 'BlockQuote',
                c: [
                    { t: 'Para', c: [{ t: 'Str', c: 'alpha' }], s: 1 },
                    { t: 'Para', c: [{ t: 'Str', c: 'gamma' }], s: 2 },
                ],
                s: 0,
            },
        ],
        astContext: { p: POOL },
    };
    return JSON.stringify(ast);
}

function mountFixture(opts: { setAst?: (ast: PandocAST) => void } = {}) {
    const setAst = opts.setAst ?? vi.fn();
    const astJson = makeAstJson();
    const props: PreviewRootProps = {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: CONTENT,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst,
        unlockNestingCursor: true,
        nestedEditBuffers: BUFFERS,
        onNavigateToDocument: () => {},
    };
    const result = render(<PreviewRoot {...props} />);
    return { ...result, setAst };
}

function mockTileRects(container: HTMLElement) {
    container.querySelectorAll<HTMLElement>('[data-block-pool-id]').forEach((tile) => {
        const pid = Number(tile.getAttribute('data-block-pool-id'));
        vi.spyOn(tile, 'getBoundingClientRect').mockReturnValue({
            left: 0, top: pid * 80, right: 300, bottom: pid * 80 + 60,
            width: 300, height: 60, x: 0, y: pid * 80, toJSON: () => ({}),
        } as DOMRect);
    });
}

/* ─────────────────────────────────────────────────────────────────────────────
 * Clean sync nest-in follows the live caret, not the frozen leafAnchorR0.
 *
 * Click Para1 (alpha) → nest-OUT to the blockquote (leafAnchorR0 stays = Para1).
 * Put the caret on line 2 ("> gamma") → nest-IN.
 * The editor must open on Para2 (gamma) — the caret-toward child — NOT Para1.
 *
 * FAIL-ON-REVERT: make requestNestingMove descend by leafAnchorR0
 * (childSurfaceToward) instead of the live caret → opens Para1 ("alpha") → RED.
 * ─────────────────────────────────────────────────────────────────────────── */
describe('§2 caret-aware nest-in (clean sync): descends toward the caret line', () => {
    it('opens the caret-toward child (gamma), not the leafAnchorR0 child (alpha)', async () => {
        const setAst = vi.fn();
        const { container } = mountFixture({ setAst });
        await act(async () => {});
        mockTileRects(container);

        // Open Para1 (alpha).
        const para1 = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(para1).not.toBeNull();
        await act(async () => {
            fireEvent(para1!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(para1!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });
        let ta = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(ta!.value).toBe('alpha');

        // Nest-out to the blockquote.
        await act(async () => {
            fireEvent.keyDown(ta!, nestingChord('ArrowLeft'));
        });
        mockTileRects(container);
        ta = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(ta!.value).toBe(BQ_SLICE);

        // Put the caret on line 2 ("> gamma"), just after "> " (offset 12).
        await act(async () => {
            ta!.focus();
            ta!.selectionStart = ta!.selectionEnd = 12;
        });

        // Nest-in → must follow the caret to Para2 (gamma).
        await act(async () => {
            fireEvent.keyDown(ta!, nestingChord('ArrowRight'));
        });
        mockTileRects(container);
        ta = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(ta).not.toBeNull();
        expect(ta!.value).toBe('gamma');           // caret-toward child, NOT 'alpha'
        // Unified caret placement maps source col 2 ("g") → dest buffer col 0.
        expect(ta!.selectionStart).toBe(0);
        // Clean nest move never commits.
        expect(setAst).not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Dirty nest-in COMMITS the edit (no data loss), then relands on the caret child.
 *
 * Open the blockquote, EDIT it (dirty), put the caret on the gamma line, nest-IN.
 * The dirty buffer must be committed (setAst called) — pre-fix the move reseeded
 * with NO commit, silently discarding the edit (the data-loss footgun). After the
 * commit re-render, the reland (kind:'nest') opens the caret-toward child (gamma).
 *
 * FAIL-ON-REVERT: make the nest move reseed without committing (skip setAst) →
 * setAst assertion fails → RED. (The data-loss footgun is exactly that revert.)
 * ─────────────────────────────────────────────────────────────────────────── */
describe('§2 commit-if-dirty: a dirty nest move commits instead of discarding the edit', () => {
    it('commits the dirty blockquote, then relands on the caret-toward child', async () => {
        const setAst = vi.fn();
        const { container, rerender } = mountFixture({ setAst });
        await act(async () => {});
        mockTileRects(container);

        // Open Para1, nest-out to the blockquote.
        const para1 = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        await act(async () => {
            fireEvent(para1!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(para1!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });
        let ta = container.querySelector<HTMLTextAreaElement>('textarea')!;
        await act(async () => {
            fireEvent.keyDown(ta, nestingChord('ArrowLeft'));
        });
        mockTileRects(container);
        ta = container.querySelector<HTMLTextAreaElement>('textarea')!;
        expect(ta.value).toBe(BQ_SLICE);

        // EDIT the blockquote (dirty): alpha → ALPHA (same length → ranges stable).
        await act(async () => {
            fireEvent.change(ta, { target: { value: '> ALPHA\n>\n> gamma' } });
        });
        // Caret on line 2 ("> gamma"), offset 12.
        await act(async () => {
            ta.focus();
            ta.selectionStart = ta.selectionEnd = 12;
        });

        // Nest-in (dirty) → must COMMIT, then close.
        await act(async () => {
            fireEvent.keyDown(ta, nestingChord('ArrowRight'));
        });
        expect(setAst).toHaveBeenCalledOnce();
        const payload = setAst.mock.calls[0][0] as unknown as { newText: string };
        expect(payload.newText).toContain('ALPHA');
        expect(container.querySelector('textarea')).toBeNull(); // editor closed

        // Simulate the commit re-render (structure preserved; alpha→ALPHA).
        const newContent = '> ALPHA\n>\n> gamma\n';
        const newAst = {
            'pandoc-api-version': [1, 23, 0],
            meta: {},
            blocks: [
                {
                    t: 'BlockQuote',
                    c: [
                        { t: 'Para', c: [{ t: 'Str', c: 'ALPHA' }], s: 1 },
                        { t: 'Para', c: [{ t: 'Str', c: 'gamma' }], s: 2 },
                    ],
                    s: 0,
                },
            ],
            astContext: { p: POOL },
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
                    nestedEditBuffers={BUFFERS}
                    onNavigateToDocument={() => {}}
                />,
            );
        });
        mockTileRects(container);

        // Reland (kind:'nest') opened the caret-toward child (gamma), not alpha.
        const relanded = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(relanded).not.toBeNull();
        expect(relanded!.value).toBe('gamma');
    });
});
