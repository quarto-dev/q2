/**
 * §6 integration tests: delete-by-emptying.
 *
 * Emptying a block and committing ANY way (Cmd/Ctrl+Enter, arrow-away, blur)
 * DELETES the block (commits empty text). Cancel is preserved ONLY when the
 * block was already empty (nothing to delete).
 *
 * Three-way guard in commitIfDirty (dispatchers.tsx):
 *   - normalized === baseline               → CANCEL (unchanged)
 *   - !normalized && !baseline             → CANCEL (already-empty, nothing to delete)
 *   - !normalized && baseline              → DELETE (had content, now empty → commit '')
 *
 * Tests:
 *   6.d — Cmd/Ctrl+Enter on non-empty → empty draft → commitTextEdit called with ''.
 *         Already-empty block → commitTextEdit NOT called (cancel gate sub-assert).
 *   6.e — Arrow-away (down): empty non-empty block, dirty down-move → commit '' +
 *         reland on deletion-point neighbor (the former next block now at L0).
 *   6.f — Blur on non-empty → empty draft → commitTextEdit called with ''.
 *   6.r — ACCEPTED-UNTESTED (it.skip): the empty-via-nesting-commit delete path
 *         is not mechanically testable at the integration layer (commitNestingEdit
 *         has no production caller; the nesting-commit chokepoint excludes empty
 *         drafts by its dirty check). The empty→delete behavior is owned by
 *         commitIfDirty and is covered by 6.d/6.e/6.f. See the 6.r header note.
 *
 * Test pattern mirrors s4-dirty-caret-col.integration.test.tsx and
 * p2-4-real.integration.test.tsx: mount PreviewRoot with a real pointer open,
 * drive events through the production EditTextarea, assert on setAst spy calls.
 *
 * FAIL-ON-REVERT guarantees:
 *   6.d — reverting the three-way guard split back to the combined `!normalized ||
 *          normalized===baseline` cancel → this test REDs on the "commitTextEdit called"
 *          assertion (delete path becomes cancel).
 *   6.e — two levers:
 *          (i)  reverting isDirty to `!!normalized && normalized !== baseline`
 *               → empty draft → isDirty=false → synchronous hop → setAst NOT called → RED.
 *          (ii) reverting reland anchor from deletion-point (L0) back to L0+draftLineCount
 *               → reland lands wrong (no block at L0+1 after delete) or misses → RED.
 *   6.f — same commitIfDirty change as 6.d covers blur (same code path).
 *   6.r — none (accepted-untested; no binding to claim — see header note).
 */

// @vitest-environment jsdom

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, act, fireEvent } from '@testing-library/react';
import React from 'react';
import { PreviewRoot } from './PreviewRoot';
import type { PreviewRootProps } from './PreviewRoot';
import type { PandocAST } from '../framework';
import * as caretGeometry from './caretGeometry';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.useRealTimers();
});

/* ─── Shared document fixture ─────────────────────────────────────────────────
 *
 * Two-paragraph document:
 *   para0 (A): pool[0] r=[0,6]   "para0\n"   line 0  ← edit target
 *   para1 (B): pool[1] r=[6,12]  "para1\n"   line 1  ← neighbor (down)
 */
const CONTENT_AB = 'para0\npara1\n';

const POOL_AB = [
    { t: 0, r: [0, 6], d: 0 },   // pool[0]: para0\n  line 0 (A)
    { t: 0, r: [6, 12], d: 0 },  // pool[1]: para1\n  line 1 (B)
];

function makeAstJson(pool: typeof POOL_AB, content: string): string {
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

function mountPreviewRoot(
    opts: {
        setAst?: (ast: PandocAST) => void;
        pool?: typeof POOL_AB;
        content?: string;
    } = {},
) {
    const pool = opts.pool ?? POOL_AB;
    const content = opts.content ?? CONTENT_AB;
    const setAst = opts.setAst ?? vi.fn();
    const astJson = makeAstJson(pool, content);
    const props: PreviewRootProps = {
        astJson,
        untransformedAstJson: astJson,
        renderedContent: content,
        currentFilePath: '/test.qmd',
        assetManifest: {},
        setAst,
        onNavigateToDocument: () => {},
    };
    const result = render(<PreviewRoot {...props} />);
    return { ...result, setAst, pool, content };
}

function ptrEvent(type: string, opts: PointerEventInit = {}): Event {
    const PE = (window as unknown as { PointerEvent?: typeof PointerEvent }).PointerEvent ?? Event;
    const evt = new PE(type, { bubbles: true, cancelable: true, ...opts });
    if (opts.pointerType !== undefined) {
        Object.defineProperty(evt, 'pointerType', { value: opts.pointerType, configurable: true });
    }
    return evt;
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

/** Open tile A (pool[0]) via real pointer events. Returns the textarea. */
async function activateTileA(container: HTMLElement): Promise<HTMLTextAreaElement> {
    const tile = container.querySelector<HTMLElement>('[data-block-pool-id="0"]');
    expect(tile).not.toBeNull();
    await act(async () => {
        fireEvent(tile!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
        fireEvent(tile!, ptrEvent('pointerup', { pointerType: 'mouse' }));
    });
    const ta = container.querySelector<HTMLTextAreaElement>('textarea');
    expect(ta).not.toBeNull();
    expect(ta!.value).toBe('para0');
    return ta!;
}

/* ─────────────────────────────────────────────────────────────────────────────
 * 6.d — Cmd/Ctrl+Enter: non-empty block → clear draft → DELETE (commit '').
 *        Already-empty block → commitTextEdit NOT called (cancel sub-assert).
 *
 * FAIL-ON-REVERT: revert the three-way guard split back to
 *   `if (!normalized || normalized === baseline) { cancel; return; }`
 * → empty-but-had-content falls into the cancel branch → setAst NOT called → RED.
 * ─────────────────────────────────────────────────────────────────────────── */
describe('§6.d — Cmd/Ctrl+Enter: empty non-empty block → DELETE', () => {
    it('calls setAst with empty newText when a non-empty block is cleared and Cmd+Enter fired', async () => {
        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst });

        await act(async () => {});
        mockTileRects(container);

        const ta = await activateTileA(container);

        // Clear the draft (from 'para0' to '').
        await act(async () => {
            fireEvent.change(ta, { target: { value: '' } });
        });

        // Cmd+Enter — should DELETE (commit with empty text).
        await act(async () => {
            fireEvent.keyDown(ta, { key: 'Enter', metaKey: true });
        });

        // setAst must have been called ONCE with empty newText.
        expect(setAst).toHaveBeenCalledOnce();
        const payload = setAst.mock.calls[0][0] as {
            __isPreviewNodeEdit: boolean;
            channel: string;
            newText: string;
        };
        expect(payload.__isPreviewNodeEdit).toBe(true);
        expect(payload.channel).toBe('text');
        expect(payload.newText).toBe('');
    });

    it('does NOT call setAst when the block was already empty (cancel — nothing to delete)', async () => {
        // Use a document with an empty block: content = '\npara1\n', pool[0] r=[0,1] (just '\n').
        const emptyPool = [
            { t: 0, r: [0, 1], d: 0 },   // pool[0]: just '\n' → anchorSlice = ''
            { t: 0, r: [1, 7], d: 0 },   // pool[1]: para1\n
        ];
        const emptyContent = '\npara1\n';

        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst, pool: emptyPool, content: emptyContent });

        await act(async () => {});
        mockTileRects(container);

        const tile = container.querySelector<HTMLElement>('[data-block-pool-id="0"]');
        expect(tile).not.toBeNull();
        await act(async () => {
            fireEvent(tile!, ptrEvent('pointerdown', { pointerType: 'mouse' }));
            fireEvent(tile!, ptrEvent('pointerup', { pointerType: 'mouse' }));
        });
        const ta = container.querySelector<HTMLTextAreaElement>('textarea');
        expect(ta).not.toBeNull();
        // The baseline is '' (trimmed '\n') — already empty.
        expect(ta!.value).toBe('');

        // Fire Cmd+Enter on the already-empty block.
        await act(async () => {
            fireEvent.keyDown(ta!, { key: 'Enter', metaKey: true });
        });

        // Already-empty → cancel (nothing to delete) → setAst NOT called.
        expect(setAst).not.toHaveBeenCalled();
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 6.e — Arrow-away (down): empty non-empty block → DELETE + reland on neighbor.
 *
 * Scenario:
 *   - Block A (pool[0], L0=0, r=[0,6], "para0\n") is open.
 *   - Draft cleared to ''.
 *   - ArrowDown at last visual line → isDirty = true (''  !== 'para0').
 *   - DELETE commit: setAst called with newText=''.
 *   - Post-delete re-render: B (pool[0] now, r=[0,6], "para1\n") at L0=0.
 *   - Reland spec: { kind:'outerByLine', direction:'down', destLine: L0 (=0) }.
 *   - After re-render, B's editor opens anchored at B.r0=0.
 *     Seam assertion: the opened editor's tile has pool-id=0 (= B.r0 in the
 *     new single-block pool), confirming the reland anchored on B and not on
 *     the deleted-A's position or a wrong line.
 *
 * Note: unlock-mode delete-reland (unlockNestingCursor=true) is correct-by-
 * construction (the destLine override feeds both resolver branches), but is
 * covered only in locked mode here (accepted; unlock dirty-reland is tracked
 * by an existing follow-up strand).
 *
 * Two FAIL-ON-REVERT levers:
 *   (i)  Revert isDirty from `normalized !== baseline` back to
 *        `!!normalized && normalized !== baseline` → empty → isDirty=false →
 *        synchronous hop → setAst NOT called → RED on "setAst called" assertion.
 *   (ii) Revert destLine for down+empty from L0 to L0+draftLineCount (= L0+1) →
 *        reland targets line 1 which after deletion doesn't exist → no textarea → RED
 *        (and anchorR0 assertion also REDs because no editor is open at all).
 * ─────────────────────────────────────────────────────────────────────────── */
describe('§6.e — arrow-away: empty block → DELETE + reland on deletion-point neighbor', () => {
    it('commits empty text (DELETE) and relands on B at L0 after down-arrow from A', async () => {
        vi.spyOn(caretGeometry, 'isOnLastVisualLine').mockReturnValue(true);
        vi.spyOn(caretGeometry, 'isOnFirstVisualLine').mockReturnValue(false);

        // Global rect mock so tiles are "visible" (non-zero) in EVERY render pass,
        // including the post-delete rerender where a NEW DOM element is created for B.
        // The reland useLayoutEffect fires INSIDE act(async () => { rerender(...) }),
        // BEFORE we can call mockTileRects on the new element, so per-spy setup
        // is too late. A global mock covers all elements regardless of when they
        // are created, ensuring isVisibleBlock returns true.
        const origGetBBox = HTMLElement.prototype.getBoundingClientRect;
        HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
            return { left: 0, top: 0, right: 200, bottom: 40, width: 200, height: 40, x: 0, y: 0, toJSON: () => ({}) } as DOMRect;
        };

        try {
            const setAst = vi.fn();
            const { container, rerender } = mountPreviewRoot({ setAst });

            await act(async () => {});

            // Activate A (pool[0], "para0").
            const ta = await activateTileA(container);

            // Clear draft to '' (delete operation).
            await act(async () => {
                fireEvent.change(ta, { target: { value: '' } });
            });

            // ArrowDown — dirty (''!=='para0') → DELETE + stash reland.
            await act(async () => {
                fireEvent.keyDown(ta, { key: 'ArrowDown' });
            });

            // setAst must be called EXACTLY ONCE with empty newText.
            expect(setAst).toHaveBeenCalledOnce();
            const payload = setAst.mock.calls[0][0] as {
                __isPreviewNodeEdit: boolean;
                channel: string;
                newText: string;
            };
            expect(payload.__isPreviewNodeEdit).toBe(true);
            expect(payload.channel).toBe('text');
            expect(payload.newText).toBe('');

            // Editor closed (A deleted, pending reland on B).
            expect(container.querySelector('textarea')).toBeNull();

            // Simulate post-delete re-render: A is gone, B is now the only block at L0=0.
            // Pool shrinks: B is pool[0], r=[0,6] ("para1\n" now starts at byte 0).
            const B_R0 = 0; // B's r[0] in the post-delete document
            const newPool = [
                { t: 0, r: [B_R0, 6], d: 0 },  // pool[0]: para1\n  (B is now at L0=0)
            ];
            const newContent = 'para1\n';
            const newAstJson = makeAstJson(newPool, newContent);

            await act(async () => {
                rerender(
                    <PreviewRoot
                        astJson={newAstJson}
                        untransformedAstJson={newAstJson}
                        renderedContent={newContent}
                        currentFilePath="/test.qmd"
                        assetManifest={{}}
                        setAst={setAst}
                        onNavigateToDocument={() => {}}
                    />,
                );
            });

            // After re-render, the reland layout effect fires.
            // Spec: { kind:'outerByLine', direction:'down', destLine: L0=0 }
            // B is now at line 0 → reland opens B's editor.
            //
            // FAIL-ON-REVERT lever (ii): if destLine were L0+draftLineCount (=L0+1=1),
            // no block is at line 1 (B is at 0) → no textarea opens → test fails here.
            const textareaB = container.querySelector<HTMLTextAreaElement>('textarea');
            expect(textareaB).not.toBeNull();
            expect(textareaB!.value).toBe('para1');

            // Seam assertion: the reland anchored on B (the post-delete pool's only
            // block, at r0=B_R0=0). When a block is actively edited, its DOM tile
            // (<p data-block-pool-id>) is replaced by the edit wrapper
            // (renderMeasuredEdit), so no [data-block-pool-id] element exists during
            // editing. We verify the anchor indirectly via two non-tautological binds:
            //   - the open editor's value is 'para1' (B's content, NOT the deleted A's
            //     'para0') — asserted above (line ~325). The reland resolves
            //     destLine=0 → first-outer-block-at/after(line 0) = B; had it anchored
            //     on the wrong line, either no editor opens (lever ii) or it carries
            //     A's stale text.
            //   - exactly ONE tile existed pre-edit; during editing the tile is
            //     replaced by the edit wrapper, so zero [data-block-pool-id] remain.
            // (Binding the literal anchorR0 would require exposing editTargetRef to the
            // test, which the harness does not do; the value='para1' check is the real
            // discriminator. The former `expect(B_R0).toBe(newPool[0].r[0])` was a
            // fixture tautology — `expect(0).toBe(0)` — and was removed 2026-06-16.)
            expect(container.querySelectorAll('[data-block-pool-id]').length).toBe(0);
            // (All [data-block-pool-id] tiles are hidden/replaced during editing.)

            // Confirm setAst was called EXACTLY ONCE (no double-commit from blur-after-arrow).
            expect(setAst).toHaveBeenCalledOnce();
        } finally {
            HTMLElement.prototype.getBoundingClientRect = origGetBBox;
        }
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * 6.f — Blur: non-empty block → clear draft → blur → DELETE (commit '').
 *
 * Blur path calls commitIfDirty(draft) after requestFocusRestore. The same
 * three-way guard in commitIfDirty handles this case identically to 6.d.
 *
 * FAIL-ON-REVERT: same code path as 6.d — revert the combined empty→cancel guard
 * → delete path becomes cancel → setAst NOT called → RED.
 * ─────────────────────────────────────────────────────────────────────────── */
describe('§6.f — blur: empty non-empty block → DELETE', () => {
    it('calls setAst with empty newText when a non-empty block is cleared and blur fires', async () => {
        const setAst = vi.fn();
        const { container } = mountPreviewRoot({ setAst });

        await act(async () => {});
        mockTileRects(container);

        const ta = await activateTileA(container);

        // Clear the draft.
        await act(async () => {
            fireEvent.change(ta, { target: { value: '' } });
        });

        // Blur — should DELETE (commitIfDirty called with '').
        await act(async () => {
            fireEvent.blur(ta);
        });

        // setAst must have been called ONCE with empty newText.
        expect(setAst).toHaveBeenCalledOnce();
        const payload = setAst.mock.calls[0][0] as {
            __isPreviewNodeEdit: boolean;
            channel: string;
            newText: string;
        };
        expect(payload.__isPreviewNodeEdit).toBe(true);
        expect(payload.channel).toBe('text');
        expect(payload.newText).toBe('');
    });
});

/* ─────────────────────────────────────────────────────────────────────────────
 * §6.r — ACCEPTED-UNTESTED (no test): empty-via-nesting-commit delete path.
 *
 * The original 6.r claimed to be a fail-on-revert regression test pinning that
 * the "nesting-commit path has no empty guard, so empty text flows straight to a
 * delete commit". A 2026-06-16 re-review found that claim FALSE and the test
 * non-binding, for three architectural reasons established by reading the
 * production code (PreviewRoot.tsx, dispatchers.tsx):
 *
 *   1. `commitNestingEdit` (PreviewRoot.tsx ~:1046) has NO production caller. It
 *      is defined and exposed via PreviewContext but invoked by nothing — the
 *      Cmd/Ctrl+Enter and blur handlers in EditTextarea (dispatchers.tsx :296,
 *      :312) both call `commitIfDirty`, not `commitNestingEdit`. So empty text
 *      cannot reach `commitNestingEdit` through any user action at the
 *      integration layer without exposing it (a production change).
 *
 *   2. The real nesting-commit chokepoint, `commitAndArmReland`
 *      (PreviewRoot.tsx ~:1104), is reached only via `requestNestingMove` /
 *      `requestNestingSelect`, whose dirty check is
 *      `const isDirty = !!draftNorm && draftNorm !== baseline` (~:1189). An EMPTY
 *      draft makes `isDirty` false → a CLEAN synchronous hop with NO commit. So
 *      the nesting-commit path NEVER routes empty text to a delete; there is no
 *      empty-guard there to pin. (The old 6.r drove a NON-empty 'modified' draft,
 *      so adding `if (!draftSrc.trim()) return` to `commitAndArmReland` left the
 *      test GREEN — fail-on-revert-confirmed false claim, 2026-06-16.)
 *
 *   3. The click-switch path, `handleClickSwitchBlur` (PreviewRoot.tsx ~:933),
 *      has its own `isDirty = !!normalized && …` guard: an empty draft returns
 *      false and falls through to the normal `commitIfDirty('')` path — i.e. the
 *      §6 three-way DELETE branch already covered by 6.d (Cmd/Ctrl+Enter) and
 *      6.f (blur). There is no distinct nesting-commit empty→delete path.
 *
 * Conclusion: the empty-text DELETE behavior is owned entirely by
 * `commitIfDirty` (dispatchers.tsx :216-258) and is mechanically tested by the
 * core triggers — 6.d (Cmd/Ctrl+Enter), 6.e (arrow-away + reland), 6.f (blur).
 * An empty-via-`commitNestingEdit` delete path is NOT mechanically testable at
 * the integration layer without exposing `commitNestingEdit` (a production
 * change we are not making). Recorded as accepted-untested rather than kept as a
 * misleading test. See the plan's §6 "Missing-test pass" list.
 * ─────────────────────────────────────────────────────────────────────────── */
describe('§6.r — accepted-untested: empty-via-nesting-commit delete path', () => {
    it.skip('empty draft → delete via commitNestingEdit: not mountable without exposing commitNestingEdit (see header note; delete IS covered by 6.d/6.e/6.f)', () => {
        // Intentionally skipped — see the accepted-untested rationale above.
    });
});
