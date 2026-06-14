/**
 * E2E tests for Plan 2b P1: edit surface no-reflow (sizing + spacing).
 *
 * The no-reflow contract has TWO parts, both asserted here:
 *   1. Sizing — the active edit region reproduces the original block's box:
 *      its height is unchanged (±1px) when the block is activated for editing.
 *   2. No reflow — the following sibling does not move: its document-relative
 *      top is unchanged (±1px) on activation.
 *   (1) catches the block's own box changing size; (2) catches margin/flow
 *   changes that a border-box height comparison alone would miss. Both are
 *   needed — losing `margin-bottom` passes (1) but fails (2).
 *
 * As-built note (measure-and-set): activating a block REPLACES it with a
 * synthetic wrapper `<div id="q2-active-edit-region">` (renderMeasuredEdit in
 * dispatchers.tsx) that reproduces the measured box; the original `<p>`/`<hN>`
 * is GONE while editing. So the "after" box is measured on
 * `#q2-active-edit-region`, never by re-querying `<tag>[data-block-pool-id]`
 * (that locator would re-resolve to a different block, or detach and time out).
 * The following-sibling locators (`<h2>`/`<ul>`) are NOT replaced, so they stay
 * valid across activation.
 */

import { test, expect, type Page, type Locator } from '@playwright/test';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

// Fixture exercises three adjacency pairs:
//   para1 → h2   (test 1: para activation, heading as the following sibling)
//   h2    → ul1  (test 2: heading activation, list as the following sibling)
//   para2 → ul2  (test 3: para activation, list as the direct following sibling)
//
// para2 and ul2 sit inside the same section as the heading, so they are
// direct siblings with no heading between them.
const FIXTURE = `---
format: q2-preview
---

This paragraph is intentionally verbose so that it spans multiple lines when rendered in
a browser. The no-reflow test verifies that the vertical space it occupies — including
its bottom margin — is exactly preserved when the block is activated for editing.

## Section Heading

- First item
- Second item

A second paragraph sitting directly above a list with no heading between them. Its
bottom margin must be preserved when editing is activated, just like the first paragraph.

- Alpha
- Beta
`;

/** Selector for the measure-and-set wrapper of the currently-active editor. */
const ACTIVE_REGION = '#q2-active-edit-region';

/** Wait for the q2-preview sourceIndex to build and editable blocks to appear. */
async function waitForEditableBlocks(page: Page): Promise<void> {
    const iframe = page.frameLocator('iframe');
    await iframe.locator('[data-block-pool-id]').first().waitFor({ timeout: 15_000 });
}

/**
 * Document-relative top of an element inside the iframe (scroll-invariant —
 * focusing the textarea on activation can scroll the iframe, which would move
 * the viewport-relative `boundingBox().y` without any reflow).
 */
async function docTop(loc: Locator): Promise<number> {
    return loc.evaluate((el) => {
        const scroller = el.ownerDocument.scrollingElement ?? el.ownerDocument.documentElement;
        return el.getBoundingClientRect().top + (scroller?.scrollTop ?? 0);
    });
}

/**
 * Drive the no-reflow contract for one adjacency case. Measures `block`'s box
 * and `sibling`'s top BEFORE activation (while their locators are still valid),
 * activates `block`, then asserts both contract parts against the active region
 * and the (unchanged) sibling.
 */
async function assertNoReflowOnActivation(
    page: Page,
    block: Locator,
    sibling: Locator,
    label: string,
): Promise<void> {
    const iframe = page.frameLocator('iframe');

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

let server: PreviewServerHandle;

test.beforeEach(async () => {
    server = await startPreviewServer({
        // Editing must be enabled or no block renders `data-block-pool-id`
        // (read-only is the default since the editingDisabled gate, 44faeb5e),
        // and these no-reflow tests wait on that attribute to activate an editor.
        allowEdit: true,
        fixtureFiles: [{ path: 'index.qmd', content: FIXTURE }],
    });
});

test.afterEach(async () => {
    await server?.stop();
});

test('paragraph: active region height and following heading top unchanged on activation', async ({ page }) => {
    await page.goto(server.url);
    await waitForEditableBlocks(page);
    const iframe = page.frameLocator('iframe');
    await assertNoReflowOnActivation(
        page,
        iframe.locator('p[data-block-pool-id]').first(),
        iframe.locator('h2').first(),
        'paragraph→heading',
    );
});

test('heading: active region height and following list top unchanged on activation', async ({ page }) => {
    await page.goto(server.url);
    await waitForEditableBlocks(page);
    const iframe = page.frameLocator('iframe');
    await assertNoReflowOnActivation(
        page,
        iframe.locator('h2[data-block-pool-id]').first(),
        iframe.locator('ul').first(),
        'heading→list',
    );
});

test('paragraph above list: active region height and list top unchanged on activation', async ({ page }) => {
    await page.goto(server.url);
    await waitForEditableBlocks(page);
    const iframe = page.frameLocator('iframe');
    await assertNoReflowOnActivation(
        page,
        iframe.locator('p[data-block-pool-id]').nth(1),
        iframe.locator('ul').nth(1),
        'paragraph→list',
    );
});
