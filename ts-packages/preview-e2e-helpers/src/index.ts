/**
 * Shared Playwright e2e helpers for q2-preview's inline-edit no-reflow contract.
 *
 * Host-agnostic: every function takes a Playwright `FrameLocator`/`Locator` for
 * the preview iframe, so it works against both hosts that embed the same
 * `@quarto/preview-renderer` — the q2-preview-spa host (real `q2 preview`
 * binary) and the hub-client host (Automerge + Monaco). Opening a document and
 * obtaining the `FrameLocator` is the caller's (host-specific) responsibility.
 *
 * The no-reflow contract has TWO parts:
 *   1. Sizing — the active editor reproduces the block's box. On activation a
 *      block is REPLACED by a synthetic measure-and-set wrapper
 *      `<div id="q2-active-edit-region">` (renderMeasuredEdit in dispatchers.tsx);
 *      the original `<p>`/`<hN>` element is gone. Measure the wrapper via
 *      `ACTIVE_REGION`, never by re-querying `<tag>[data-block-pool-id]` (that
 *      locator would re-resolve to a different block, or detach and time out).
 *   2. No reflow — surrounding content does not move on activation. The
 *      following-sibling locators (`<h2>`/`<ul>`) are NOT replaced, so they stay
 *      valid across activation.
 *
 * `assertNoReflowOnActivation` is the lean form (sizing + first following
 * sibling). `measureLayout`/`expectLayoutStable` are the broad form (every
 * block's top + document height) for callers that want full-document coverage.
 */
import { expect, type FrameLocator, type Locator } from '@playwright/test';

/** Selector for the measure-and-set wrapper of the currently-active editor. */
export const ACTIVE_REGION = '#q2-active-edit-region';

/**
 * Document-relative top of an element inside the iframe (scroll-invariant —
 * focusing the textarea on activation can scroll the iframe, which would move
 * the viewport-relative `boundingBox().y` without any reflow).
 */
export async function docTop(loc: Locator): Promise<number> {
    return loc.evaluate((el) => {
        const scroller = el.ownerDocument.scrollingElement ?? el.ownerDocument.documentElement;
        return el.getBoundingClientRect().top + (scroller?.scrollTop ?? 0);
    });
}

/** Click the first block matching `tag`, returning its pool id; waits for the textarea. */
export async function activateBlock(iframe: FrameLocator, tag: string): Promise<string> {
    const target = iframe.locator(`${tag}[data-block-pool-id]`).first();
    const poolId = await target.getAttribute('data-block-pool-id');
    await target.click();
    await iframe.locator('textarea').first().waitFor({ timeout: 5000 });
    return poolId!;
}

/**
 * Layout snapshot for the broad zero-reflow invariant: the viewport top of every
 * editable block (keyed by its stable pool id) plus the document's total
 * scrollHeight. Activating a block for editing must not move ANY other block
 * and must not change the document's total height (no "space crunch").
 */
export type Layout = { tops: Record<string, number>; scrollHeight: number };

export async function measureLayout(iframe: FrameLocator): Promise<Layout> {
    // Settle before reading geometry: wait for web fonts to finish loading
    // (font swaps change line heights) and two animation frames so any pending
    // reflow has flushed. Without this, parallel workers can snapshot `before`
    // mid-font-load and see spurious sub-block shifts.
    await iframe.locator('body').evaluate(async () => {
        await (document as Document & { fonts?: FontFaceSet }).fonts?.ready;
        await new Promise<void>((r) =>
            requestAnimationFrame(() => requestAnimationFrame(() => r())),
        );
    });
    const tops = await iframe.locator('[data-block-pool-id]').evaluateAll((els) =>
        Object.fromEntries(
            els.map((el) => [
                el.getAttribute('data-block-pool-id')!,
                el.getBoundingClientRect().top,
            ]),
        ),
    );
    const scrollHeight = await iframe
        .locator('body')
        .evaluate(() => document.documentElement.scrollHeight);
    return { tops, scrollHeight };
}

/**
 * Assert that going `before → after` (activation of `editedPoolId`) moved no
 * surviving block by more than `tol` px and did not change the document height.
 * Reports every offending block + delta so a failure shows exactly where the
 * crunch is (space above/below a heading, between a paragraph and a list, ...).
 */
export function expectLayoutStable(
    before: Layout,
    after: Layout,
    editedPoolId: string,
    tol = 1.5,
): void {
    const moved: string[] = [];
    for (const [poolId, top] of Object.entries(after.tops)) {
        if (poolId === editedPoolId) continue;
        const delta = Math.abs(top - before.tops[poolId]);
        if (delta > tol) moved.push(`block ${poolId} moved ${delta.toFixed(1)}px`);
    }
    const heightDelta = Math.abs(after.scrollHeight - before.scrollHeight);
    expect(moved, `blocks shifted on activation:\n  ${moved.join('\n  ')}`).toEqual([]);
    expect(
        heightDelta,
        `document height changed by ${heightDelta.toFixed(1)}px on activation (space crunch)`,
    ).toBeLessThanOrEqual(tol);
}

/**
 * Lean no-reflow assertion for one adjacency case. Asserts BOTH contract parts:
 * (1) the active region's height matches `block`'s pre-activation height
 * (sizing, measured on `ACTIVE_REGION`), and (2) `sibling`'s document top is
 * unchanged (no reflow). `block`/`sibling` must be resolved from `iframe`; they
 * are measured before activation while their locators are still valid.
 */
export async function assertNoReflowOnActivation(
    iframe: FrameLocator,
    block: Locator,
    sibling: Locator,
    label: string,
): Promise<void> {
    // Before: block box + sibling top (locators valid pre-activation).
    const blockBox = await block.boundingBox();
    expect(blockBox, `${label}: block must be visible before activation`).not.toBeNull();
    const siblingTopBefore = await docTop(sibling);

    // Activate; wait for the textarea to mount.
    await block.click();
    await iframe.locator('textarea').first().waitFor({ timeout: 5_000 });

    // After: measure the active region (the original block element is gone).
    const region = iframe.locator(ACTIVE_REGION);
    const regionBox = await region.boundingBox();
    expect(regionBox, `${label}: active edit region must exist after activation`).not.toBeNull();
    const siblingTopAfter = await docTop(sibling);

    // Part 1 (sizing): the active region reproduces the block's box height.
    expect(
        Math.abs(regionBox!.height - blockBox!.height),
        `${label}: active region height changed from ${blockBox!.height}px to ${regionBox!.height}px on activation`,
    ).toBeLessThanOrEqual(1);

    // Part 2 (no reflow): the following sibling does not move.
    expect(
        Math.abs(siblingTopAfter - siblingTopBefore),
        `${label}: sibling top shifted by ${Math.abs(siblingTopAfter - siblingTopBefore).toFixed(1)}px on activation ` +
            `(was ${siblingTopBefore.toFixed(1)}px, now ${siblingTopAfter.toFixed(1)}px); margin-bottom was likely lost`,
    ).toBeLessThanOrEqual(1);
}
