/**
 * bd-9x3zbuj8 Task 2 / bd-igpm0xur — nesting breadcrumb folded into the pop-up
 * EditToolbar.
 *
 * When the rich-text editor is showing for the active target (a Para/Header in
 * rich mode), the nesting breadcrumb renders INLINE in the toolbar row
 * (`.q2-rt-toolbar .q2-crumb`), to the right of the formatting buttons — NOT as a
 * separate floating chip (`[data-testid="q2-breadcrumb-chip"]`), which would
 * overlap the toolbar.
 *
 * After the edit-chrome consolidation there is NO standalone floating chip: a
 * NON-rich block (e.g. a CodeBlock) gets the SAME folded toolbar hosting the
 * breadcrumb — just without the rich mode toggle / marks.
 *
 * Drives the REAL PreviewRoot so the dispatcher picks the surface and the real
 * EditToolbar / RichTextEditor mount (tiptap runs in jsdom).
 *
 * Structural-overlap note: asserting the crumbs live INSIDE `.q2-rt-toolbar`
 * proves they flow after the toolbar buttons in the same flex row, so the
 * toolbar's left edge is independent of the breadcrumb's width — the no-overlap
 * invariant. Pixel geometry (toolbar-left stable as crumb width changes) is
 * jsdom-untestable (all rects are 0) and is covered by Playwright.
 */

// @vitest-environment jsdom

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, act, fireEvent } from '@testing-library/react';
import { PreviewRoot } from './PreviewRoot';
import type { PreviewRootProps } from './PreviewRoot';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.useRealTimers();
});

/* ─── PointerEvent helper (verbatim from p3-4-breadcrumb) ───────────────────── */
function ptrEvent(
    type: string,
    opts: PointerEventInit & { clientX?: number; clientY?: number } = {},
): Event {
    const PE = (window as any).PointerEvent ?? Event;
    const evt = new PE(type, { bubbles: true, cancelable: true, ...opts });
    for (const [key, val] of Object.entries({
        ...(opts.pointerType !== undefined ? { pointerType: opts.pointerType } : {}),
    } as Record<string, unknown>)) {
        Object.defineProperty(evt, key, { value: val, configurable: true });
    }
    return evt;
}

/* ─── Fixture: a Para nested in a Div (so the path has > 1 crumb) ───────────── */
// content bytes (length 25): :::␠d\nAAA\n\nBBB\n:::\npara2\n
const CONTENT = '::: d\nAAA\n\nBBB\n:::\npara2\n';
const POOL = [
    { t: 0, r: [0, 18], d: 0 },    // pool[0] Div        siKey 0:0-18:0
    { t: 0, r: [6, 9], d: 0 },     // pool[1] ParaA      siKey 0:6-9:0
    { t: 0, r: [11, 14], d: 0 },   // pool[2] ParaB      siKey 0:11-14:0
    { t: 0, r: [19, 24], d: 0 },   // pool[3] para2      siKey 0:19-24:0
];

function makeAstJson(): string {
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks: [
            {
                t: 'Div',
                c: [
                    ['', ['d'], []],
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

function mountFixture(opts: { richText?: boolean } = {}) {
    const astJson = makeAstJson();
    const props: PreviewRootProps = {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: CONTENT,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst: vi.fn(),
        unlockNestingCursor: true,
        richText: opts.richText,
        onNavigateToDocument: () => {},
    };
    return render(<PreviewRoot {...props} />);
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

async function openEditor(container: HTMLElement, poolId: string) {
    const el = container.querySelector<HTMLElement>(`[data-block-pool-id="${poolId}"]`)!;
    await act(async () => {
        fireEvent(el, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(el, ptrEvent('pointerup', { pointerType: 'mouse' }));
    });
}

const standaloneChip = (c: HTMLElement) =>
    c.querySelector<HTMLElement>('[data-testid="q2-breadcrumb-chip"]');
const toolbar = (c: HTMLElement) => c.querySelector<HTMLElement>('.q2-rt-toolbar');

describe('bd-9x3zbuj8 Task 2 — inline breadcrumb in the rich-text toolbar', () => {
    it('renders the breadcrumb INSIDE the toolbar row and suppresses the standalone chip (rich Para)', async () => {
        const { container } = mountFixture({ richText: true });
        await act(async () => {});
        mockTileRects(container);

        // Open ParaB (pool-id=2) — a rich-text-supported Para nested in a Div.
        await openEditor(container, '2');

        // The rich-text toolbar must be present (rich editor is the active surface).
        const tb = toolbar(container);
        expect(tb, 'rich-text toolbar must render for a Para in rich mode').not.toBeNull();

        // The crumbs must live INSIDE the toolbar row — proving side-by-side layout
        // with the formatting buttons (no separate floating chip to overlap).
        const inlineCrumbs = Array.from(tb!.querySelectorAll<HTMLElement>('.q2-crumb'));
        expect(inlineCrumbs.map((c) => c.textContent)).toEqual(['Dv', '¶']);
        // The in/out nav arrows are inline too.
        expect(tb!.querySelector('.q2-breadcrumb-out')).not.toBeNull();
        expect(tb!.querySelector('.q2-breadcrumb-in')).not.toBeNull();

        // The standalone floating chip must NOT render for this target (no double).
        expect(
            standaloneChip(container),
            'standalone chip must be suppressed when the inline breadcrumb is showing',
        ).toBeNull();
    });

    it('uses the SAME folded toolbar (no standalone chip) for a non-rich block (CodeBlock)', async () => {
        // A CodeBlock is not rich-text-supported, so the textarea (not the rich
        // editor) opens — but the breadcrumb now folds into the pop-up toolbar too
        // (the standalone floating chip was retired). Exactly one toolbar, no chip,
        // and no rich chrome (no mode toggle / marks).
        const codeContent = '``` python\nx = 1\n```\n';
        const codePool = [{ t: 0, r: [0, 20], d: 0 }]; // pool[0] CodeBlock
        const codeAst = JSON.stringify({
            'pandoc-api-version': [1, 23, 0],
            meta: {},
            blocks: [
                { t: 'CodeBlock', c: [['', ['python'], []], 'x = 1'], s: 0 },
            ],
            astContext: { p: codePool },
        });
        const props: PreviewRootProps = {
            astJson: codeAst,
            untransformedAstJson: codeAst,
            renderedContent: codeContent,
            currentFilePath: '/code.qmd',
            assetManifest: {},
            setAst: vi.fn(),
            unlockNestingCursor: true,
            richText: true, // on, but a CodeBlock is unsupported → plain surface, folded toolbar
            onNavigateToDocument: () => {},
        };
        const { container } = render(<PreviewRoot {...props} />);
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '0'); // the CodeBlock

        // The folded toolbar hosts the breadcrumb for a CodeBlock too.
        const tb = toolbar(container);
        expect(tb, 'the folded toolbar renders for a CodeBlock').not.toBeNull();
        // Nesting on → full breadcrumb with ◀/▶ (path is just the CodeBlock crumb).
        expect(Array.from(tb!.querySelectorAll('.q2-crumb')).map((c) => c.textContent)).toEqual(['Cd']);
        expect(tb!.querySelector('.q2-breadcrumb-out')).not.toBeNull();
        // No rich chrome for a non-rich block.
        expect(tb!.querySelector('.q2-rt-tb-mode'), 'no mode toggle for a CodeBlock').toBeNull();
        // The standalone floating chip is retired.
        expect(
            standaloneChip(container),
            'standalone chip must NOT render (it is retired)',
        ).toBeNull();
    });
});
