/**
 * bd-7pxub583 — a tight-list item (a `Plain` block) opens the RICH-TEXT editor.
 *
 * Tight bullet/ordered lists store each item's content as a `Plain` block. Before
 * this change `RICHTEXT_SUPPORTED_TYPES` was {Para, Header}, so clicking a
 * tight-list item resolved a `Plain` sourceNode, `richTextAvailable` returned
 * false, and the block fell back to the monospaced textarea. This drives the REAL
 * PreviewRoot (so the dispatcher makes the real surface choice and the tiptap
 * editor mounts in jsdom) and asserts the rich-text toolbar is present for a
 * clicked tight-list item — the observable signal that the rich editor, not the
 * textarea, is the active surface.
 *
 * Mirrors the harness in p3-4-inline-breadcrumb.integration.test.tsx.
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

/* ─── Fixture: a tight 2-item bullet list (items are Plain) ─────────────────── */
// content bytes: "- apple\n- banana\n"
//   "apple"  → r=[2, 7]      "banana" → r=[10, 16]      BulletList → r=[0, 17]
const CONTENT = '- apple\n- banana\n';
const POOL = [
    { t: 0, r: [2, 7], d: 0 },   // pool[0] Plain "apple"  (borrowed onto <li>)
    { t: 0, r: [10, 16], d: 0 }, // pool[1] Plain "banana"
    { t: 0, r: [0, 17], d: 0 },  // pool[2] BulletList
];

function makeAstJson(): string {
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks: [
            {
                t: 'BulletList',
                s: 2,
                c: [
                    [{ t: 'Plain', s: 0, c: [{ t: 'Str', c: 'apple' }] }],
                    [{ t: 'Plain', s: 1, c: [{ t: 'Str', c: 'banana' }] }],
                ],
            },
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
        // Unlocked so a click resolves to the innermost item (the Plain), rather
        // than climbing to the whole <ul>.
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

const toolbar = (c: HTMLElement) => c.querySelector<HTMLElement>('.q2-rt-toolbar');
const textarea = (c: HTMLElement) => c.querySelector<HTMLElement>('textarea');

describe('bd-7pxub583 — tight-list item (Plain) opens the rich-text editor', () => {
    it('opens the RICH editor (toolbar present) when clicking a tight-list item with richText on', async () => {
        const { container } = mountFixture({ richText: true });
        await act(async () => {});
        mockTileRects(container);

        // The <li> borrows the leading Plain's pool-id (=0); clicking it targets
        // the Plain block.
        await openEditor(container, '0');

        expect(
            toolbar(container),
            'rich-text toolbar must render for a Plain tight-list item in rich mode',
        ).not.toBeNull();
        // And it is the rich surface, not the fallback textarea.
        expect(textarea(container), 'no monospaced textarea when the rich editor is active').toBeNull();
    });

    it('falls back to the textarea (no toolbar) for the same item when richText is off', async () => {
        const { container } = mountFixture({ richText: false });
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '0');

        expect(toolbar(container), 'no rich toolbar when the flag is off').toBeNull();
        expect(
            textarea(container),
            'the textarea is the edit surface when richText is off',
        ).not.toBeNull();
    });
});
