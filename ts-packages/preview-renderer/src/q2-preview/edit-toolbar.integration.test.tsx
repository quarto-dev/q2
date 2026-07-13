/**
 * bd-igpm0xur — the single pop-up EditToolbar is the consistent edit-chrome host
 * for EVERY editable block.
 *
 * Consolidation contract (this file is the primary regression net):
 *   - One `.q2-rt-toolbar` renders per edit session, for rich blocks AND plain
 *     blocks (code chunks, etc.).
 *   - The rich/plain choice is a single `.q2-rt-tb-mode` (Markdown-mark) icon on
 *     the toolbar, shown only when the block is rich-supported.
 *   - Formatting marks render only on the rich surface (editor mounted).
 *   - A type/nesting indicator ALWAYS shows (min = the current-type crumb; the
 *     full ◀/▶ ancestor path when unlockNestingCursor is on).
 *   - The old left-margin `.q2-edit-affordance` ("Editing…") is gone, and so is
 *     the standalone floating `[data-testid="q2-breadcrumb-chip"]`.
 *
 * Drives the REAL PreviewRoot so the dispatcher makes the real surface choice
 * and the real EditToolbar / RichTextEditor mount (tiptap runs in jsdom).
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

/* ─── PointerEvent helper (verbatim from p3-4-inline-breadcrumb) ────────────── */
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

async function mountAndOpen(
    astJson: string,
    content: string,
    poolId: string,
    opts: { richText?: boolean; unlockNestingCursor?: boolean } = {},
) {
    const props: PreviewRootProps = {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: content,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst: vi.fn(),
        richText: opts.richText,
        unlockNestingCursor: opts.unlockNestingCursor,
        onNavigateToDocument: () => {},
    };
    const utils = render(<PreviewRoot {...props} />);
    await act(async () => {});
    mockTileRects(utils.container);
    await openEditor(utils.container, poolId);
    return { ...utils, setAst: props.setAst as ReturnType<typeof vi.fn> };
}

/* ─── Fixtures ──────────────────────────────────────────────────────────────── */

// Single top-level paragraph.
const PARA_CONTENT = 'AAA\n';
const PARA_AST = JSON.stringify({
    'pandoc-api-version': [1, 23, 0],
    meta: {},
    blocks: [{ t: 'Para', c: [{ t: 'Str', c: 'AAA' }], s: 0 }],
    astContext: { p: [{ t: 0, r: [0, 3], d: 0 }] },
});

// Single top-level code block.
const CODE_CONTENT = '``` python\nx = 1\n```\n';
const CODE_AST = JSON.stringify({
    'pandoc-api-version': [1, 23, 0],
    meta: {},
    blocks: [{ t: 'CodeBlock', c: [['', ['python'], []], 'x = 1'], s: 0 }],
    astContext: { p: [{ t: 0, r: [0, 20], d: 0 }] },
});

// A Para (pool-id 2) nested in a Div — the multi-crumb nesting fixture.
const NEST_CONTENT = '::: d\nAAA\n\nBBB\n:::\npara2\n';
const NEST_AST = JSON.stringify({
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
    astContext: {
        p: [
            { t: 0, r: [0, 18], d: 0 },
            { t: 0, r: [6, 9], d: 0 },
            { t: 0, r: [11, 14], d: 0 },
            { t: 0, r: [19, 24], d: 0 },
        ],
    },
});

const toolbars = (c: HTMLElement) => c.querySelectorAll<HTMLElement>('.q2-rt-toolbar');
const modeToggle = (c: HTMLElement) => c.querySelector<HTMLElement>('.q2-rt-tb-mode');
const boldMark = (c: HTMLElement) => c.querySelector<HTMLElement>('.q2-rt-tb-bold');
const standaloneChip = (c: HTMLElement) => c.querySelector<HTMLElement>('[data-testid="q2-breadcrumb-chip"]');

describe('bd-igpm0xur — one pop-up toolbar for every editable block', () => {
    it('rich Para (nesting off): one toolbar w/ toggle + marks + ¶ crumb; no affordance', async () => {
        const { container } = await mountAndOpen(PARA_AST, PARA_CONTENT, '0', { richText: true });

        expect(toolbars(container)).toHaveLength(1);
        const tb = toolbars(container)[0];
        // Mode toggle (rich-supported) + formatting marks (rich editor mounted).
        expect(modeToggle(container), 'mode toggle shows for a rich-supported block').not.toBeNull();
        expect(boldMark(tb), 'mark buttons render on the rich surface').not.toBeNull();
        // Type indicator: the current-type crumb for a Para is ¶.
        const crumbs = Array.from(tb.querySelectorAll('.q2-crumb'));
        expect(crumbs.map((c) => c.textContent)).toEqual(['¶']);
        // The retired left-margin affordance and its "Editing…" label are gone.
        expect(container.querySelector('.q2-edit-affordance')).toBeNull();
        expect(container.textContent).not.toContain('Editing');
    });

    it('code block (nesting off): one toolbar; NO toggle; NO marks; Cd crumb', async () => {
        const { container } = await mountAndOpen(CODE_AST, CODE_CONTENT, '0', { richText: true });

        expect(toolbars(container)).toHaveLength(1);
        const tb = toolbars(container)[0];
        // Not rich-supported → no mode toggle; plain surface → no marks.
        expect(modeToggle(container), 'no mode toggle for a non-rich block').toBeNull();
        expect(boldMark(tb), 'no mark buttons on the plain surface').toBeNull();
        // The type crumb's VISIBLE text is the abbrev "Cd" (the language/label lives
        // in the title/tooltip, not the crumb text).
        const crumbs = Array.from(tb.querySelectorAll('.q2-crumb'));
        expect(crumbs.map((c) => c.textContent)).toEqual(['Cd']);
        expect(container.querySelector('.q2-edit-affordance')).toBeNull();
    });

    it('nesting on (rich Para): toolbar carries the full breadcrumb (◀/▶); no standalone chip', async () => {
        const { container } = await mountAndOpen(NEST_AST, NEST_CONTENT, '2', {
            richText: true,
            unlockNestingCursor: true,
        });

        expect(toolbars(container)).toHaveLength(1);
        const tb = toolbars(container)[0];
        // Full ancestor path with nesting nav.
        const crumbs = Array.from(tb.querySelectorAll('.q2-crumb'));
        expect(crumbs.map((c) => c.textContent)).toEqual(['Dv', '¶']);
        expect(tb.querySelector('.q2-breadcrumb-out')).not.toBeNull();
        expect(tb.querySelector('.q2-breadcrumb-in')).not.toBeNull();
        // The standalone floating chip is retired.
        expect(standaloneChip(container)).toBeNull();
    });

    it('regression: richText OFF + nesting ON still renders one toolbar w/ full breadcrumb, no toggle', async () => {
        const { container } = await mountAndOpen(NEST_AST, NEST_CONTENT, '2', {
            richText: false,
            unlockNestingCursor: true,
        });

        expect(toolbars(container)).toHaveLength(1);
        const tb = toolbars(container)[0];
        // Rich off → no mode toggle; but the full breadcrumb still shows (this is the
        // case a naïve `ctx.richText`-only gate would have dropped).
        expect(modeToggle(container), 'rich off → no toggle').toBeNull();
        const crumbs = Array.from(tb.querySelectorAll('.q2-crumb'));
        expect(crumbs.map((c) => c.textContent)).toEqual(['Dv', '¶']);
        expect(tb.querySelector('.q2-breadcrumb-out')).not.toBeNull();
        expect(standaloneChip(container)).toBeNull();
    });

    it('toggle: mode icon swaps rich→plain (marks gone, textarea appears) without committing; back to rich', async () => {
        const { container, setAst } = await mountAndOpen(PARA_AST, PARA_CONTENT, '0', { richText: true });

        // Rich surface: marks present, no textarea.
        expect(boldMark(container)).not.toBeNull();
        expect(container.querySelector('textarea')).toBeNull();

        // Click the mode toggle → swap to plain.
        await act(async () => {
            fireEvent.mouseDown(modeToggle(container)!);
        });

        // Plain surface: still exactly one toolbar, toggle now pressed, marks gone,
        // textarea present, and NO commit happened (a swap is not a "done" gesture).
        expect(toolbars(container)).toHaveLength(1);
        expect(modeToggle(container)!.getAttribute('aria-pressed')).toBe('true');
        expect(boldMark(container), 'marks disappear on the plain surface').toBeNull();
        expect(container.querySelector('textarea')).not.toBeNull();
        expect(setAst).not.toHaveBeenCalled();

        // Toggle back → rich surface returns (marks back, textarea gone).
        await act(async () => {
            fireEvent.mouseDown(modeToggle(container)!);
        });
        expect(modeToggle(container)!.getAttribute('aria-pressed')).toBe('false');
        expect(boldMark(container), 'marks return on the rich surface').not.toBeNull();
        expect(container.querySelector('textarea')).toBeNull();
    });
});
