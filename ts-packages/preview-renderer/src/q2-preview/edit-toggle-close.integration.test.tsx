/**
 * Live edit toggle: switching `editingDisabled` on while an edit session is
 * open must close the session through the editor's own commit path
 * (bd-ew0vak6b, plan decision D6).
 *
 * Until now `editingDisabled` was fixed for the life of a host session
 * (`q2 preview` without `--allow-edit`), so nothing reacted to it changing.
 * hub-client's bottom-bar Edit pill flips it live. The dispatcher's
 * edit-target gate (`isBlockEditTarget`) does not consult the flag, so an
 * open editor stays mounted with its draft intact when the flag turns on —
 * which is exactly what lets the editor commit as a blur would:
 *
 *   - dirty textarea  → commit once via setAst, then close;
 *   - unchanged       → close without a commit;
 *   - rich editor     → same contract through its own `commit`;
 *   - after the close, no block advertises editability, and turning editing
 *     back on restores the affordance.
 *
 * Real production path through PreviewRoot (same harness shape as
 * p2-4d.integration.test.tsx). jsdom gotchas handled the same way: tile
 * rects are mocked so enumerateOuterBlocks sees the tiles, and PointerEvent
 * pointerType is forced via defineProperty.
 */

// @vitest-environment jsdom

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, act, fireEvent } from '@testing-library/react';
import React from 'react';
import { PreviewRoot } from './PreviewRoot';
import type { PreviewRootProps } from './PreviewRoot';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
});

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

// Two paragraphs: pool[0] "para0\n" and pool[1] "para1\n".
const CONTENT = 'para0\npara1\n';
const POOL = [
    { t: 0, r: [0, 6], d: 0 },
    { t: 0, r: [6, 12], d: 0 },
];

function makeAstJson(): string {
    const blocks = POOL.map((entry, i) => {
        const text = CONTENT.slice(entry.r[0], entry.r[1]).replace(/\n/g, '');
        return { t: 'Para', c: [{ t: 'Str', c: text }], s: i };
    });
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks,
        astContext: { p: POOL },
    });
}

function makeProps(overrides: Partial<PreviewRootProps> = {}): PreviewRootProps {
    const astJson = makeAstJson();
    return {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: CONTENT,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst: vi.fn(),
        onNavigateToDocument: () => {},
        ...overrides,
    };
}

function mockTileRects(container: HTMLElement) {
    container.querySelectorAll<HTMLElement>('[data-block-pool-id]').forEach((tile) => {
        const pid = Number(tile.getAttribute('data-block-pool-id'));
        vi.spyOn(tile, 'getBoundingClientRect').mockReturnValue({
            left: 0, top: pid * 60, right: 200, bottom: pid * 60 + 40,
            width: 200, height: 40, x: 0, y: pid * 60, toJSON: () => ({}),
        } as DOMRect);
    });
}

async function openEditor(container: HTMLElement, poolId: string) {
    const tile = container.querySelector<HTMLElement>(`[data-block-pool-id="${poolId}"]`);
    expect(tile, `tile ${poolId} must be editable before the toggle`).not.toBeNull();
    await act(async () => {
        fireEvent(tile!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(tile!, ptrEvent('pointerup', { pointerType: 'mouse' }));
    });
}

const textarea = (c: HTMLElement) => c.querySelector<HTMLTextAreaElement>('textarea');
const richToolbar = (c: HTMLElement) => c.querySelector<HTMLElement>('.q2-rt-toolbar');
const poolTiles = (c: HTMLElement) => c.querySelectorAll('[data-block-pool-id]');

describe('editingDisabled flipped on while editing (bd-ew0vak6b, D6)', () => {
    it('commits a dirty textarea draft exactly once and closes the session', async () => {
        const setAst = vi.fn();
        const props = makeProps({ setAst });
        const { container, rerender } = render(<PreviewRoot {...props} />);
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '1');
        const ta = textarea(container);
        expect(ta).not.toBeNull();
        expect(ta!.value).toBe('para1');

        await act(async () => {
            fireEvent.change(ta!, { target: { value: 'para1 edited' } });
        });
        expect(setAst).not.toHaveBeenCalled();

        // The host turns editing off (the Edit pill).
        await act(async () => {
            rerender(<PreviewRoot {...props} editingDisabled={true} />);
        });

        expect(setAst, 'the dirty draft is committed, once').toHaveBeenCalledOnce();
        const payload = setAst.mock.calls[0][0] as any;
        expect(payload.__isPreviewNodeEdit).toBe(true);
        expect(payload.channel).toBe('text');
        expect(payload.newText).toContain('para1 edited');

        expect(textarea(container), 'the editor is closed').toBeNull();
        expect(poolTiles(container).length, 'no block advertises editability').toBe(0);
    });

    it('closes an unchanged textarea session without committing', async () => {
        const setAst = vi.fn();
        const props = makeProps({ setAst });
        const { container, rerender } = render(<PreviewRoot {...props} />);
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '0');
        expect(textarea(container)).not.toBeNull();

        await act(async () => {
            rerender(<PreviewRoot {...props} editingDisabled={true} />);
        });

        expect(setAst).not.toHaveBeenCalled();
        expect(textarea(container)).toBeNull();
        expect(poolTiles(container).length).toBe(0);
    });

    it('closes an unchanged rich-text session without committing', async () => {
        const setAst = vi.fn();
        const props = makeProps({ setAst, richText: true });
        const { container, rerender } = render(<PreviewRoot {...props} />);
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '1');
        expect(richToolbar(container), 'the rich editor is the surface for a Para').not.toBeNull();

        await act(async () => {
            rerender(<PreviewRoot {...props} editingDisabled={true} />);
        });

        expect(setAst).not.toHaveBeenCalled();
        expect(richToolbar(container), 'the rich editor is closed').toBeNull();
        expect(poolTiles(container).length).toBe(0);
    });

    it('turning editing back on restores the affordance and a new session can open', async () => {
        const setAst = vi.fn();
        const props = makeProps({ setAst });
        const { container, rerender } = render(<PreviewRoot {...props} />);
        await act(async () => {});
        mockTileRects(container);

        await openEditor(container, '1');
        expect(textarea(container)).not.toBeNull();

        await act(async () => {
            rerender(<PreviewRoot {...props} editingDisabled={true} />);
        });
        expect(textarea(container)).toBeNull();
        expect(poolTiles(container).length).toBe(0);

        await act(async () => {
            rerender(<PreviewRoot {...props} editingDisabled={false} />);
        });
        expect(poolTiles(container).length).toBe(2);
        mockTileRects(container);

        await openEditor(container, '0');
        expect(textarea(container)).not.toBeNull();
        expect(textarea(container)!.value).toBe('para0');
    });
});
