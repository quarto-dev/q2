/**
 * P3.4 §3d integration tests: BreadcrumbChip floating chip — breadcrumb
 * rendering, ◀/▶ nesting navigation, and crumb-click jump-to-level.
 *
 * All tests drive the REAL PreviewRoot (no custom registry) so real Div/Para
 * components render nested [data-block-pool-id] elements and the real chip
 * renders. The REAL context callbacks are driven via the REAL chip
 * buttons/crumbs. No re-target logic is re-implemented here.
 *
 * Fixture:
 * ────────
 * content = '::: d\nAAA\n\nBBB\n:::\npara2\n'  (25 bytes)
 *
 * Byte map:
 *   0 :  1 :  2 :  3 ␠  4 d   5 \n
 *   6 A  7 A  8 A  9 \n 10 \n
 *  11 B 12 B 13 B 14 \n
 *  15 : 16 : 17 : 18 \n
 *  19 p 20 a 21 r 22 a 23 2 24 \n
 *
 * pool:
 *   pool[0] Div        {t:0, r:[0,18],  d:0}   siKey '0:0-18:0'   slice '::: d\nAAA\n\nBBB\n:::'
 *   pool[1] ParaA      {t:0, r:[6,9],   d:0}   siKey '0:6-9:0'    slice 'AAA'
 *   pool[2] ParaB      {t:0, r:[11,14], d:0}   siKey '0:11-14:0'  slice 'BBB'
 *   pool[3] para2      {t:0, r:[19,24], d:0}   siKey '0:19-24:0'  slice 'para2'
 *
 * Ancestor path for cursor=(11,14) [ParaB]:
 *   Div.d  (r=[0,18], isCurrent=false)
 *   Para   (r=[11,14], isCurrent=true)
 *
 * Event-isolation deferral (→ P3.5 Playwright):
 *   The rigorous stopPropagation / preventDefault behaviour for chip buttons
 *   (preventing host-level click-switch and blur-commit) depends on real browser
 *   pointer-event sequencing, real focus/blur, and real DOM hit-testing — none of
 *   which jsdom simulates. fireEvent.click fires no pointer events and does not
 *   move focus. Therefore NO jsdom test asserts "chip click does not switch
 *   blocks" (that assertion would be vacuous / not fail-on-revert). The production
 *   code DOES implement stopPropagation/preventDefault correctly for the real
 *   browser. Pointer-isolation testing is deferred to P3.5 Playwright.
 */

// @vitest-environment jsdom

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, act, fireEvent } from '@testing-library/react';
import React from 'react';
import { PreviewRoot } from './PreviewRoot';
import type { PreviewRootProps } from './PreviewRoot';
import { detectPlatform } from './nestingNav';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.useRealTimers();
});

/* ─── Platform detection ──────────────────────────────────────────────────── */
// (detectPlatform used in tests 5 for the tooltip labels; not needed for click
// tests but kept for symmetry with p3-3-nesting)
const _PLATFORM = detectPlatform();
void _PLATFORM; // suppress unused-var lint

/* ─── PointerEvent helper (verbatim from p3-3-nesting) ─────────────────────── */
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

/* ─── Fixture ─────────────────────────────────────────────────────────────── */

// content bytes (length 25):
// :::␠d\nAAA\n\nBBB\n:::\npara2\n
const CONTENT = '::: d\nAAA\n\nBBB\n:::\npara2\n';

const POOL = [
    { t: 0, r: [0, 18], d: 0 },    // pool[0] Div        siKey 0:0-18:0
    { t: 0, r: [6, 9], d: 0 },     // pool[1] ParaA      siKey 0:6-9:0
    { t: 0, r: [11, 14], d: 0 },   // pool[2] ParaB      siKey 0:11-14:0
    { t: 0, r: [19, 24], d: 0 },   // pool[3] para2      siKey 0:19-24:0
];

const DIV_SLICE = '::: d\nAAA\n\nBBB\n:::';

function makeAstJson(): string {
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks: [
            {
                t: 'Div',
                c: [
                    ['', ['d'], []],  // Attr: class "d" → label "Div.d"
                    [
                        { t: 'Para', c: [{ t: 'Str', c: 'AAA' }], s: 1 },
                        { t: 'Para', c: [{ t: 'Str', c: 'BBB' }], s: 2 },
                    ],
                ],
                s: 0,
            },
            { t: 'Para', c: [{ t: 'Str', c: 'para2' }], s: 3 },
        ],
        astContext: { p: POOL },
    });
}

function mountFixture(opts: { setAst?: any; unlockNestingCursor?: boolean } = {}) {
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
        onNavigateToDocument: () => {},
    };
    return { ...render(<PreviewRoot {...props} />), setAst };
}

/** Open the editor on a given pool-id via real pointer events. */
async function openEditor(container: HTMLElement, poolId: string) {
    const el = container.querySelector<HTMLElement>(`[data-block-pool-id="${poolId}"]`)!;
    await act(async () => {
        fireEvent(el, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(el, ptrEvent('pointerup', { pointerType: 'mouse' }));
    });
}

/** Mock getBoundingClientRect on all [data-block-pool-id] tiles. */
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

const chip = (c: HTMLElement) => c.querySelector<HTMLElement>('[data-testid="q2-breadcrumb-chip"]');
const ta = (c: HTMLElement) => c.querySelector<HTMLTextAreaElement>('textarea');

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 1 ★ (fail-on-revert): Chip hidden when LOCKED
 *
 * unlockNestingCursor is NOT set; open editor on ParaB (pool-id=2).
 * Editor opens (textarea appears) but chip does NOT appear.
 *
 * FAIL-ON-REVERT: removing the unlockNestingCursor gate in BreadcrumbChip
 * makes the chip appear when locked → chip(container) !== null → test fails.
 * ─────────────────────────────────────────────────────────────────────────── */
describe('P3.4 test 1 ★ — chip hidden when locked', () => {
    it('chip does not render when unlockNestingCursor is not set', async () => {
        const { container } = mountFixture({}); // no unlockNestingCursor
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '2'); // ParaB

        expect(ta(container)).not.toBeNull(); // editor IS open
        expect(chip(container)).toBeNull();   // chip is NOT shown
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 2: Chip hidden when NO editor open
 *
 * unlockNestingCursor=true but no editor open → chip null.
 * ─────────────────────────────────────────────────────────────────────────── */
describe('P3.4 test 2 — chip hidden when no editor open', () => {
    it('chip does not render when editTarget is null', async () => {
        const { container } = mountFixture({ unlockNestingCursor: true });
        await act(async () => {});

        expect(chip(container)).toBeNull();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 3: Chip shown when unlocked + editor open
 * ─────────────────────────────────────────────────────────────────────────── */
describe('P3.4 test 3 — chip shown when unlocked + editor open', () => {
    it('chip renders when unlockNestingCursor=true and editor is open', async () => {
        const { container } = mountFixture({ unlockNestingCursor: true });
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '2'); // ParaB

        const chipEl = chip(container);
        expect(chipEl).not.toBeNull();
        // Forward-crumb placeholder must exist (guards successor plan's drop-in contract).
        expect(chipEl!.querySelector('.q2-breadcrumb-future')).not.toBeNull();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 4: Ancestor path rendered
 *
 * unlocked, open ParaB (cursor=(11,14)).
 * Chip crumb buttons (.q2-crumb) must read ['Div.d', 'Para'] in order.
 * Last crumb (Para) has aria-current="true".
 * ─────────────────────────────────────────────────────────────────────────── */
describe('P3.4 test 4 — ancestor path rendered in chip', () => {
    it('crumb buttons show [Div.d, Para] with Para as aria-current', async () => {
        const { container } = mountFixture({ unlockNestingCursor: true });
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '2'); // ParaB — cursor=(11,14)

        const chipEl = chip(container);
        expect(chipEl).not.toBeNull();

        const crumbs = Array.from(chipEl!.querySelectorAll<HTMLElement>('.q2-crumb'));
        // Crumbs show abbreviated glyphs as textContent.
        expect(crumbs.map(c => c.textContent)).toEqual(['Dv', '¶']);

        // Full labels are exposed via title and aria-label attributes.
        expect(crumbs[0].getAttribute('title')).toBe('Div.d');
        expect(crumbs[0].getAttribute('aria-label')).toBe('Div.d');
        expect(crumbs[1].getAttribute('title')).toBe('Para');
        expect(crumbs[1].getAttribute('aria-label')).toBe('Para');

        // Category class on the Div crumb.
        expect(crumbs[0].className).toContain('q2-crumb-cat-container');

        // Current node (Para) carries aria-current="true"
        expect(crumbs[1].getAttribute('aria-current')).toBe('true');
        // Non-current node (Div.d) must NOT have aria-current
        expect(crumbs[0].getAttribute('aria-current')).toBeNull();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 5 ★ (fail-on-revert): ◀ / ▶ buttons move the nesting cursor
 *
 * unlocked, open ParaB (value 'BBB').
 * Click ◀ (q2-breadcrumb-out) → editor re-targets to Div → value = DIV_SLICE.
 * Click ▶ (q2-breadcrumb-in) → editor descends back to ParaB → value = 'BBB'.
 * setAst NOT called.
 *
 * FAIL-ON-REVERT: neutralize the ◀ onClick (or requestNestingMove) → editor stays
 * at 'BBB' → test fails.
 * ─────────────────────────────────────────────────────────────────────────── */
describe('P3.4 test 5 ★ — ◀/▶ buttons move the nesting cursor', () => {
    it('out button moves to Div, in button moves back to ParaB', async () => {
        const setAst = vi.fn();
        const { container } = mountFixture({ setAst, unlockNestingCursor: true });
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '2'); // ParaB — value 'BBB'
        expect(ta(container)!.value).toBe('BBB');

        // Click ◀ (nesting out: ParaB → Div)
        const outBtn = chip(container)!.querySelector<HTMLElement>('.q2-breadcrumb-out')!;
        await act(async () => { fireEvent.click(outBtn); });
        mockTileRects(container);

        expect(ta(container)!.value).toBe(DIV_SLICE);

        // Click ▶ (nesting in: Div → ParaB, toward leafAnchorR0=11)
        const inBtn = chip(container)!.querySelector<HTMLElement>('.q2-breadcrumb-in')!;
        await act(async () => { fireEvent.click(inBtn); });
        mockTileRects(container);

        expect(ta(container)!.value).toBe('BBB');

        // Nesting moves must NOT commit
        expect(setAst).not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 6 ★ (fail-on-revert): after a crumb jump, ▶ follows the live CARET
 *
 * CONTRACT CHANGE (§2, 2026-06-15): nest-in now descends toward the live caret's
 * source line, NOT the frozen `leafAnchorR0`. The pre-§2 version of this test
 * asserted the opposite ("jump to Div then ▶ still descends to ParaB because
 * leafAnchorR0=11"); that invariant is intentionally retired — the live cursor is
 * the single source of truth (Reflection #5). `leafAnchorR0` survives only as the
 * no-readable-caret fallback.
 *
 * unlocked, open ParaB (value 'BBB', leafAnchorR0=11).
 * Click the 'Div.d' crumb → requestNestingSelect(0,18) → value = DIV_SLICE.
 * Place the caret on the 'AAA' line (line 1 of the Div buffer) → click ▶ →
 * descends to ParaA (the caret-toward child) → value 'AAA', overriding the
 * leafAnchorR0=11 (ParaB) that the old contract would have followed.
 *
 * FAIL-ON-REVERT: make nest-in descend by `leafAnchorR0` instead of the caret
 * → ▶ hits ParaB → value 'BBB' ≠ 'AAA' → RED.
 * ─────────────────────────────────────────────────────────────────────────── */
describe('P3.4 test 6 ★ — after a crumb jump, ▶ follows the live caret (not leafAnchorR0)', () => {
    it('jump to Div.d, caret on the AAA line, then ▶ descends to ParaA (caret wins over leafAnchorR0)', async () => {
        const setAst = vi.fn();
        const { container } = mountFixture({ setAst, unlockNestingCursor: true });
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '2'); // ParaB — value 'BBB', leafAnchorR0=11
        expect(ta(container)!.value).toBe('BBB');

        // Click the 'Div.d' crumb → jump to (0,18)
        const chipEl = chip(container)!;
        const divCrumb = Array.from(chipEl.querySelectorAll<HTMLElement>('.q2-crumb'))
            .find(c => c.getAttribute('title') === 'Div.d')!;
        expect(divCrumb).not.toBeUndefined();

        await act(async () => { fireEvent.click(divCrumb); });
        mockTileRects(container);
        expect(ta(container)!.value).toBe(DIV_SLICE);

        // Place the caret on the 'AAA' line (buffer line 1 = offset 6, after "::: d\n").
        await act(async () => {
            const t = ta(container)!;
            t.focus();
            t.selectionStart = t.selectionEnd = 6;
        });

        // Click ▶ — must follow the caret to ParaA ('AAA'), overriding leafAnchorR0=11.
        const inBtn = chip(container)!.querySelector<HTMLElement>('.q2-breadcrumb-in')!;
        await act(async () => { fireEvent.click(inBtn); });
        mockTileRects(container);
        expect(ta(container)!.value).toBe('AAA');

        // Neither the jump nor the (clean) nest-in commits.
        expect(setAst).not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 7 ★ (fail-on-revert): a DIRTY crumb jump COMMITS, then relands on the target.
 *
 * §2 commit-if-dirty must cover crumb-jumps too (the ▶/◀ buttons preventDefault, so
 * the textarea never blur-commits): editing ParaB then clicking the 'Div.d' crumb
 * must commit the edit (was: reseed with no commit — silent data loss) and reland
 * on the chosen ancestor (Div) via resolveLanding kind:'crumb'.
 *
 * FAIL-ON-REVERT: make requestNestingSelect reseed without committing → setAst
 * assertion fails → RED.
 * ─────────────────────────────────────────────────────────────────────────── */
describe('P3.4 test 7 ★ — a dirty crumb jump commits, then relands on the target', () => {
    it('edits ParaB, clicks Div.d → commits, relands on the Div', async () => {
        const setAst = vi.fn();
        const { container, rerender } = mountFixture({ setAst, unlockNestingCursor: true });
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '2'); // ParaB — value 'BBB'
        let textarea = ta(container)!;
        expect(textarea.value).toBe('BBB');

        // Edit dirty (length-preserving so ranges stay stable): BBB → CCC.
        await act(async () => {
            fireEvent.change(textarea, { target: { value: 'CCC' } });
        });

        // Click the 'Div.d' crumb → must COMMIT, then close.
        const divCrumb = Array.from(chip(container)!.querySelectorAll<HTMLElement>('.q2-crumb'))
            .find(c => c.getAttribute('title') === 'Div.d')!;
        await act(async () => { fireEvent.click(divCrumb); });
        expect(setAst).toHaveBeenCalledOnce();
        const payload = setAst.mock.calls[0][0] as unknown as { newText: string };
        expect(payload.newText).toContain('CCC');
        expect(ta(container)).toBeNull(); // editor closed

        // Simulate the commit re-render (ParaB BBB→CCC, ranges preserved).
        const newContent = '::: d\nAAA\n\nCCC\n:::\npara2\n';
        const newAst = {
            'pandoc-api-version': [1, 23, 0],
            meta: {},
            blocks: [
                {
                    t: 'Div',
                    c: [
                        ['', ['d'], []],
                        [
                            { t: 'Para', c: [{ t: 'Str', c: 'AAA' }], s: 1 },
                            { t: 'Para', c: [{ t: 'Str', c: 'CCC' }], s: 2 },
                        ],
                    ],
                    s: 0,
                },
                { t: 'Para', c: [{ t: 'Str', c: 'para2' }], s: 3 },
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
                    onNavigateToDocument={() => {}}
                />,
            );
        });
        mockTileRects(container);

        // Reland (kind:'crumb') opened the Div (the chosen ancestor), with CCC.
        textarea = ta(container)!;
        expect(textarea).not.toBeNull();
        expect(textarea.value).toBe('::: d\nAAA\n\nCCC\n:::');
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * Test 8 ★ (T18 — fail-on-revert): code-block-in-blockquote shows BOTH crumbs
 *
 * Fixture: a CodeBlock nested inside a BlockQuote.
 * QMD source: '> ```\n> code here\n> ```\n'  (20 bytes)
 *
 * Byte map:
 *   0  > ␠ ` ` `  \n
 *   6  > ␠ c  o  d  e  ␠  h  e  r  e  \n
 *  18  > ␠ ` ` `  \n   (note: entire blockquote is bytes 0–20)
 *
 * The pool entries mirror the AST structure:
 *   pool[0] BlockQuote  r=[0,20]  (the outer container)
 *   pool[1] CodeBlock   r=[0,20]  (the code block, same range in this fixture
 *                                  because the blockquote wraps it exactly)
 *
 * NOTE: In jsdom, surfaceLeft <= 0 (all geometry returns 0), so the
 * computeChipGeometry branch `surfaceLeft <= 0 → slots = crumbCount` fires.
 * This means geometry changes (left-spill vs gutter-only) do NOT affect
 * whether crumbs render in jsdom — BOTH old and new geometry take the same
 * full-path branch. T18 therefore guards:
 *   (a) buildAncestorPath correctly returns BlockQuote AND CodeBlock ancestors
 *   (b) selectDisplayItems renders the full path (both crumbs visible)
 *
 * FAIL-ON-REVERT: break buildAncestorPath (e.g. drop BlockQuote from the
 * ancestor walk) → only CodeBlock crumb present → getByTitle('BlockQuote')
 * assertion fails → RED. A geometry revert does NOT redden this test (both
 * old/new geometry take the same jsdom branch).
 * ─────────────────────────────────────────────────────────────────────────── */
describe('T18 ★ — code-block-in-blockquote: both crumbs render in jsdom (guards buildAncestorPath + selectDisplayItems)', () => {
    // Fixture: a BlockQuote containing a CodeBlock.
    // Source: "> ```\n> code here\n> ```\n"  (21 bytes)
    //
    // Byte layout:
    //   0  '>'  1  ' '  2  '`'  3  '`'  4  '`'  5  '\n'
    //   6  '>'  7  ' '  8  'c'  9  'o' 10  'd' 11  'e' 12  ' ' 13  'h' 14  'e' 15  'r' 16  'e' 17  '\n'
    //  18  '>'  19 ' '  20 '`'  21 '`'  22 '`'  23 '\n'
    //  Total: 24 bytes
    //
    // AST structure:
    //   BlockQuote (pool[0], r=[0,24])
    //     CodeBlock (pool[1], r=[0,24]) — same range (blockquote wraps it exactly)
    //
    // The pool ranges are equal: buildAncestorPath must emit BOTH entries
    // (deduplication key is "r0:r1", but BlockQuote ≠ CodeBlock so they differ
    // in the seen-map key... Wait, they'd have the same dedupeKey "0:24"!
    // Use a slightly different range for the CodeBlock to avoid deduplication.)
    //
    // REVISED: Use distinct ranges to avoid the dedupeKey collision:
    //   BlockQuote r=[0,24] (outer, wider range)
    //   CodeBlock  r=[2,22] (inner, just the fenced code without outer > marks)
    //
    // For the purposes of this test the exact byte values don't need to match
    // a real parse — we just need buildAncestorPath to find BOTH ancestors when
    // the cursor is on the CodeBlock range.

    const BQ_CONTENT = '> ```\n> code here\n> ```\n';   // 24 bytes
    const BQ_R0 = 0, BQ_R1 = 24;
    const CB_R0 = 2, CB_R1 = 22;   // inner range (distinct from BlockQuote)
    // cursorR0/R1 = CodeBlock's range (the active surface)
    const CURSOR_R0 = CB_R0, CURSOR_R1 = CB_R1;

    function makeBqAstJson(): string {
        return JSON.stringify({
            'pandoc-api-version': [1, 23, 0],
            meta: {},
            blocks: [
                {
                    t: 'BlockQuote',
                    c: [
                        {
                            t: 'CodeBlock',
                            c: [
                                ['', [], []],  // Attr (empty)
                                'code here',   // code text
                            ],
                            s: 1,  // pool index 1
                        },
                    ],
                    s: 0,  // pool index 0
                },
            ],
            astContext: {
                p: [
                    { t: 0, r: [BQ_R0, BQ_R1], d: 0 },   // pool[0] BlockQuote
                    { t: 0, r: [CB_R0, CB_R1], d: 0 },    // pool[1] CodeBlock
                ],
            },
        });
    }

    it('chip shows both BlockQuote and CodeBlock crumbs when editing the code block', async () => {
        const astJson = makeBqAstJson();
        const props: PreviewRootProps = {
            astJson,
            untransformedAstJson: astJson,
            renderedContent: BQ_CONTENT,
            currentFilePath: '/test-bq.qmd',
            assetManifest: {},
            setAst: vi.fn(),
            unlockNestingCursor: true,
            onNavigateToDocument: () => {},
        };
        const { container, getByTitle } = render(<PreviewRoot {...props} />);
        await act(async () => {});

        // Mock tile rects so the editor can be activated.
        const tiles = container.querySelectorAll<HTMLElement>('[data-block-pool-id]');
        tiles.forEach((tile) => {
            const pid = Number(tile.getAttribute('data-block-pool-id'));
            vi.spyOn(tile, 'getBoundingClientRect').mockReturnValue({
                left: 0, top: pid * 80, right: 300, bottom: pid * 80 + 60,
                width: 300, height: 60, x: 0, y: pid * 80, toJSON: () => ({}),
            } as DOMRect);
        });

        // Open the CodeBlock editor (pool-id=1, r=[CB_R0, CB_R1]).
        const codeBlockEl = container.querySelector<HTMLElement>('[data-block-pool-id="1"]');
        expect(codeBlockEl, 'CodeBlock element must be in the DOM').not.toBeNull();

        await act(async () => {
            fireEvent(codeBlockEl!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(codeBlockEl!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });

        // The textarea must be open.
        expect(ta(container), 'textarea must open for CodeBlock').not.toBeNull();

        // The chip must be visible (unlockNestingCursor=true).
        const chipEl = chip(container);
        expect(chipEl, 'BreadcrumbChip must render when editor is open').not.toBeNull();

        // CORE ASSERTION (T18 fail-on-revert):
        // Both the BlockQuote ancestor AND the CodeBlock current crumb must be present.
        // getByTitle throws if the element is not found — that's the RED condition.
        //
        // Failure mode: if buildAncestorPath drops BlockQuote (e.g. skips non-current
        // ancestors), only 'CodeBlock' would render → getByTitle('BlockQuote') throws → RED.
        expect(
            getByTitle('BlockQuote'),
            'A crumb with title="BlockQuote" must render — guards buildAncestorPath returning the ancestor',
        ).not.toBeNull();
        expect(
            getByTitle('CodeBlock'),
            'A crumb with title="CodeBlock" must render — guards selectDisplayItems returning the current node',
        ).not.toBeNull();
    });
});
