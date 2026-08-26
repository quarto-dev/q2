/**
 * bd-84ljmbaf / GH #420 — the open editor must SURVIVE (not remount across)
 * an external re-render that shifts the active block's byte offsets.
 *
 * A collaborator's CRDT change anywhere BEFORE the active block changes the
 * block's `r[0]`. Before the fix, the first render after such a change still
 * carried the stale `editTarget.anchorR0`, so the dispatcher's anchor match
 * failed and the editor unmounted for one render; the P2.3b self-heal layout
 * effect then re-anchored and a SECOND render mounted a fresh editor. The
 * existing p2-3b-real KEEP tests assert "editor stays open / draft preserved"
 * — which a remount satisfies (the textarea reseeds from editDraftRef) — so
 * they cannot catch the remount itself. Its user-visible cost: a focus blip
 * (keystrokes dropped), caret reset to end, and for the RICH surface loss of
 * the uncommitted tiptap doc (it reseeds from the AST, not the draft).
 *
 * These tests pin the stronger contract: across an offset-shifting external
 * re-render the editor's DOM element is IDENTICAL (`toBe`), and focus never
 * leaves it. Element identity is the precise observable — a remount cannot
 * preserve it — and it is what guarantees caret/selection/undo/doc survival.
 *
 * Fix under test: PreviewRoot derives the re-anchored edit target DURING
 * render (`effectiveEditTarget`), so the first post-change render already
 * matches the new offsets. See
 * claude-notes/plans/2026-08-26-gh420-editor-focus-crdt.md.
 *
 * Out of scope (follow-ups filed from bd-84ljmbaf): whole-block insert/delete
 * above the active block (index-keyed `key={i}` still remounts), and remote
 * edits to the active block itself (self-heal DROP closes the editor).
 *
 * Harness mirrors p2-3b-real.integration.test.tsx (real PreviewRoot, real
 * pointer-event activation) + plain-list-item-richtext.integration.test.tsx
 * (rich surface via `richText: true`).
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
    vi.useRealTimers();
});

/* ─── PointerEvent helper (verbatim from p2-3b-real) ────────────────────────── */
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

/* ─── Pool / content helpers (shape shared with p2-3b-real) ─────────────────── */

type PoolEntry = { t: 0; r: [number, number]; d: 0 };
type Pool = readonly PoolEntry[];

function makeAstJson(pool: Pool, content: string): string {
    const blocks = pool.map((entry, i) => {
        const raw = content.slice(entry.r[0], entry.r[1]);
        const text = raw.replace(/\n/g, '').trim() || `tile${i}`;
        return { t: 'Para', c: [{ t: 'Str', c: text }], s: i };
    });
    return JSON.stringify({
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks,
        astContext: { p: pool },
    });
}

function buildProps(pool: Pool, content: string, richText: boolean): PreviewRootProps {
    const astJson = makeAstJson(pool, content);
    return {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: content,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst: vi.fn(),
        richText,
        onNavigateToDocument: () => {},
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

async function activateTile(container: HTMLElement, poolId: number) {
    const tile = container.querySelector<HTMLElement>(`[data-block-pool-id="${poolId}"]`);
    expect(tile, `tile with pool-id ${poolId} should be in DOM`).not.toBeNull();
    await act(async () => {
        fireEvent(tile!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(tile!, ptrEvent('pointerup', { pointerType: 'mouse' }));
    });
}

/** Flush one animation frame (RichTextEditor places its opening caret in rAF). */
async function flushRaf() {
    await act(async () => {
        await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    });
}

/* ─── Fixtures ──────────────────────────────────────────────────────────────── */

// Base doc, three paragraphs. The edit target is para1 (pool[1]).
//   "para0\npara1\npara2\n\n"
const BASE_CONTENT = 'para0\npara1\npara2\n\n';
const BASE_POOL: Pool = [
    { t: 0, r: [0, 6], d: 0 },   // "para0\n"
    { t: 0, r: [6, 12], d: 0 },  // "para1\n"  ← active editor
    { t: 0, r: [12, 19], d: 0 }, // "para2\n\n"
];

// Collaborator GROWS para0 ("para0" → "para0-grown-longer"): every later
// block shifts FORWARD (+13 bytes). para1's content is unchanged.
const GROWN_CONTENT = 'para0-grown-longer\npara1\npara2\n\n';
const GROWN_POOL: Pool = [
    { t: 0, r: [0, 19], d: 0 },  // "para0-grown-longer\n"
    { t: 0, r: [19, 25], d: 0 }, // "para1\n"  (shifted +13)
    { t: 0, r: [25, 32], d: 0 }, // "para2\n\n"
];

// Collaborator SHRINKS para0 ("para0" → "p0"): every later block shifts
// BACK (−3 bytes) — the direction a strictly at/after re-anchor would miss.
const SHRUNK_CONTENT = 'p0\npara1\npara2\n\n';
const SHRUNK_POOL: Pool = [
    { t: 0, r: [0, 3], d: 0 },  // "p0\n"
    { t: 0, r: [3, 9], d: 0 },  // "para1\n"  (shifted −3)
    { t: 0, r: [9, 16], d: 0 }, // "para2\n\n"
];

// Collaborator edits para2 (AFTER the active block): para1's offsets are
// untouched. This always worked; it validates the harness baseline so the
// shift tests fail for the right reason.
const AFTER_CONTENT = 'para0\npara1\npara2-edited\n\n';
const AFTER_POOL: Pool = [
    { t: 0, r: [0, 6], d: 0 },
    { t: 0, r: [6, 12], d: 0 },
    { t: 0, r: [12, 26], d: 0 }, // "para2-edited\n\n"
];

const pmEl = (c: HTMLElement) => c.querySelector<HTMLElement>('.tiptap.ProseMirror');

/* ─── Rich surface ──────────────────────────────────────────────────────────── */

describe('bd-84ljmbaf — rich editor survives offset-shifting external re-render', () => {
    async function openRichOnPara1(pool: Pool, content: string) {
        const props = buildProps(pool, content, true);
        const view = render(<PreviewRoot {...props} />);
        await act(async () => {});
        mockTileRects(view.container);
        await activateTile(view.container, 1);
        await flushRaf(); // opening caret placement (focus('end') fallback)
        const pm = pmEl(view.container);
        expect(pm, 'rich editor should open on para1').not.toBeNull();
        return { view, pm: pm! };
    }

    function externalRerender(
        view: ReturnType<typeof render>,
        pool: Pool,
        content: string,
    ) {
        const next = buildProps(pool, content, true);
        act(() => {
            view.rerender(<PreviewRoot {...next} />);
        });
    }

    it('baseline: edit AFTER the active block — same element, focus kept', async () => {
        const { view, pm } = await openRichOnPara1(BASE_POOL, BASE_CONTENT);
        pm.focus();
        expect(document.activeElement).toBe(pm);

        externalRerender(view, AFTER_POOL, AFTER_CONTENT);

        const pmAfter = pmEl(view.container);
        expect(pmAfter).toBe(pm);
        expect(document.activeElement).toBe(pm);
    });

    it('forward shift (collaborator grows an earlier block): SAME element, focus kept', async () => {
        const { view, pm } = await openRichOnPara1(BASE_POOL, BASE_CONTENT);
        pm.focus();
        expect(document.activeElement).toBe(pm);

        externalRerender(view, GROWN_POOL, GROWN_CONTENT);

        const pmAfter = pmEl(view.container);
        expect(pmAfter, 'rich editor should still be open').not.toBeNull();
        // The identity assertion is the point: a remount (unmount + fresh
        // mount) can re-open an editor but never return the same element.
        expect(pmAfter).toBe(pm);
        // Focus must never have left the editor (the GH #420 symptom).
        expect(document.activeElement).toBe(pm);
    });

    it('backward shift (collaborator shrinks an earlier block): SAME element, focus kept', async () => {
        const { view, pm } = await openRichOnPara1(BASE_POOL, BASE_CONTENT);
        pm.focus();
        expect(document.activeElement).toBe(pm);

        externalRerender(view, SHRUNK_POOL, SHRUNK_CONTENT);

        const pmAfter = pmEl(view.container);
        expect(pmAfter, 'rich editor should still be open').not.toBeNull();
        expect(pmAfter).toBe(pm);
        expect(document.activeElement).toBe(pm);
    });
});

/* ─── Plain textarea surface ────────────────────────────────────────────────── */

describe('bd-84ljmbaf — textarea survives offset-shifting external re-render', () => {
    async function openTextareaOnPara1() {
        const props = buildProps(BASE_POOL, BASE_CONTENT, false);
        const view = render(<PreviewRoot {...props} />);
        await act(async () => {});
        mockTileRects(view.container);
        await activateTile(view.container, 1);
        const ta = view.container.querySelector<HTMLTextAreaElement>('textarea');
        expect(ta, 'textarea should open on para1').not.toBeNull();
        return { view, ta: ta! };
    }

    it('forward shift: SAME element, dirty draft and focus kept', async () => {
        const { view, ta } = await openTextareaOnPara1();
        ta.focus();
        // Dirty the draft — trivially possible on the textarea surface.
        await act(async () => {
            fireEvent.change(ta, { target: { value: 'para1 EDITED-LOCALLY' } });
        });
        expect(document.activeElement).toBe(ta);

        const next = buildProps(GROWN_POOL, GROWN_CONTENT, false);
        act(() => {
            view.rerender(<PreviewRoot {...next} />);
        });

        const taAfter = view.container.querySelector<HTMLTextAreaElement>('textarea');
        expect(taAfter, 'textarea should still be open').not.toBeNull();
        expect(taAfter).toBe(ta);
        expect(taAfter!.value).toBe('para1 EDITED-LOCALLY');
        expect(document.activeElement).toBe(ta);
    });
});
