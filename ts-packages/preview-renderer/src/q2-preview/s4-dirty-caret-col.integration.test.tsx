/**
 * §4 integration test: dirty nest-in/out caret-column consistency (Principle A).
 *
 * Principle A: same source line/column going IN and OUT with the nesting cursor.
 * The CLEAN nest path maps (Ls, Cs) via `cleanCaretHint` / `prefixWidth`.
 * Before this fix, the DIRTY path stored the raw `live.bufferCol` in the
 * ResolverSpec and placed it verbatim on the destination — no `prefixWidth`
 * adjustment — so a clean vs dirty nest-in could differ by the prefix width (e.g. 2
 * for a blockquote's "> " prefix).
 *
 * Fixture:
 *   content = '> line one\n> line two\n'   (22 bytes)
 *     line 0: '> line one'   bytes 0–9   (\n @10)
 *     line 1: '> line two'   bytes 11–20 (\n @21)
 *
 *   pool[0] BlockQuote {t:0, r:[0,22], d:0}   siKey '0:0-22:0'  (verbatim, no clean buffer)
 *   pool[1] ChildPara  {t:0, r:[2,21], d:0}   siKey '0:2-21:0'  clean buffer 'line one\nline two'
 *
 * prefixWidth for ChildPara on source line 0:
 *   fullT = '> line one', cleanT = 'line one' → delta = 2
 * prefixWidth for BlockQuote (verbatim) = 0
 *
 * Clean nest-in column mapping at BQ bufferCol=5, line=0:
 *   Cs = 5 + prefixWidth(BQ=0) = 5
 *   destCol = 5 − prefixWidth(ChildPara=2) = 3
 *
 * FAIL-ON-REVERT: remove the prefixWidth adjustment from resolveLanding 'nest'
 * → destCol = spec.caretBufferCol = 5 → selectionStart=5 ≠ 3 → RED.
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

// content = '> line one\n> line two\n'  (22 bytes)
//   line 0: '> line one'  bytes 0–9  (\n @10)
//   line 1: '> line two'  bytes 11–20 (\n @21)
const CONTENT = '> line one\n> line two\n';

const POOL = [
    { t: 0, r: [0, 22], d: 0 },  // pool[0]: BlockQuote (verbatim, no clean buffer)
    { t: 0, r: [2, 21], d: 0 },  // pool[1]: ChildPara
];

// clean buffer for the child para (strips the '> ' prefix from each line)
const CHILD_SI_KEY = '0:2-21:0';
const CLEAN_BUFFER = 'line one\nline two';

// BQ anchorSlice = content[0..22].trimEnd() = '> line one\n> line two'
const BQ_ANCHOR_SLICE = '> line one\n> line two';

const BUFFERS: Record<string, string> = { [CHILD_SI_KEY]: CLEAN_BUFFER };

function makeAstJson(): string {
    // The `s` pool-index fields on blocks are a runtime extension the
    // framework types don't declare (readers cast per-site); cast once here.
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
        ],
        astContext: { p: POOL },
    } as unknown as PandocAST;
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
 * §4 — dirty nest-in caret column is adjusted by prefixWidth (Principle A).
 *
 * Steps:
 *   1. Open child para → nest-OUT (clean) → blockquote editor opens.
 *   2. Put caret at BQ buffer col 5, line 0 (the "e" in "> line one").
 *   3. Edit BQ to make it dirty (change "two" → "TWO" on line 1; keep line 0
 *      intact so the prefixWidth suffix-check passes for source line 0).
 *   4. Nest-IN (dirty path): commits the BQ edit, closes, then relands on the
 *      child para via resolveLanding kind:'nest'.
 *   5. After reland, the child para's selectionStart must equal 3 (= source
 *      col 5 − prefixWidth 2), NOT 5 (the raw BQ bufferCol).
 *
 * FAIL-ON-REVERT: in resolveLanding 'nest', replace the prefixWidth-adjusted
 * destCol with `spec.caretBufferCol` → selectionStart=5 ≠ 3 → RED.
 * ─────────────────────────────────────────────────────────────────────────── */
describe('§4 — dirty nest-in: caret column is adjusted by prefixWidth (Principle A)', () => {
    it('places the caret at source col 5 − prefixWidth(2) = col 3 in the child buffer after dirty round-trip', async () => {
        const setAst = vi.fn();
        const { container, rerender } = mountFixture({ setAst });
        await act(async () => {});
        mockTileRects(container);

        // ── Step 1: open child para, then nest-OUT to the blockquote ─────────
        const childTile = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(childTile).not.toBeNull();
        await act(async () => {
            fireEvent(childTile!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(childTile!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });
        let ta = container.querySelector<HTMLTextAreaElement>('textarea')!;
        expect(ta.value).toBe(CLEAN_BUFFER);

        await act(async () => {
            fireEvent.keyDown(ta, nestingChord('ArrowLeft'));
        });
        mockTileRects(container);
        ta = container.querySelector<HTMLTextAreaElement>('textarea')!;
        expect(ta.value).toBe(BQ_ANCHOR_SLICE);

        // ── Step 2: put caret at BQ buffer col 5, line 0 ────────────────────
        // BQ buffer = '> line one\n> line two'
        // col 5 on line 0 → "e" in "> line one" (0='>' 1=' ' 2='l' 3='i' 4='n' 5='e')
        await act(async () => {
            ta.focus();
            ta.selectionStart = ta.selectionEnd = 5; // col 5 on line 0
        });

        // ── Step 3: make the BQ dirty (change "two" → "TWO" on line 1) ──────
        // We keep line 0 ("> line one") unchanged so prefixWidth's suffix-check
        // passes when computing the adjustment for source line 0.
        await act(async () => {
            fireEvent.change(ta, { target: { value: '> line one\n> line TWO' } });
        });
        // Restore caret to col 5 line 0 after the change event.
        await act(async () => {
            ta.selectionStart = ta.selectionEnd = 5;
        });

        // Confirm dirty (setAst not yet called).
        expect(setAst).not.toHaveBeenCalled();

        // ── Step 4: nest-IN (dirty) → must commit then close ─────────────────
        await act(async () => {
            fireEvent.keyDown(ta, nestingChord('ArrowRight'));
        });
        expect(setAst).toHaveBeenCalledOnce();
        expect(container.querySelector('textarea')).toBeNull(); // editor closed

        // ── Step 5: simulate the commit re-render ────────────────────────────
        // Content stays structurally identical (ranges stable), only "two"→"TWO".
        const newContent = '> line one\n> line TWO\n';
        const newAstJson = JSON.stringify({
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
                                { t: 'Str', c: 'line TWO' },
                            ],
                            s: 1,
                        },
                    ],
                    s: 0,
                },
            ],
            astContext: { p: POOL },
        });
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

        // ── Assertion: child para opened with prefixWidth-adjusted column ─────
        // source col Cs = BQ bufferCol(5) + prefixWidth(BQ=verbatim)=0 = 5
        // dest bufferCol = Cs − prefixWidth(ChildPara=2) = 5 − 2 = 3
        // selectionStart = 3 (within 'line one', the 'e' in "lin|e one")
        //
        // BEFORE FIX: resolveLanding used spec.caretBufferCol=5 verbatim → selectionStart=5
        // AFTER  FIX: prefixWidth adjustment → selectionStart=3
        const relanded = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(relanded).not.toBeNull();
        expect(relanded!.value).toBe(CLEAN_BUFFER);
        expect(relanded!.selectionStart).toBe(3); // col 3 = source col 5 − prefixWidth(2)
        expect(relanded!.selectionEnd).toBe(3);
    });
});
