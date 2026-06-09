/**
 * E2E tests for Plan 2b P1: edit surface no-reflow (sizing + spacing).
 *
 * Acceptance criteria (§P1 of the Plan 2b design prerequisites):
 *   - Activating a block for editing does NOT shift the following sibling
 *     (i.e. the document does not reflow). Specifically:
 *       sibling.getBoundingClientRect().top is unchanged (±1px).
 *   - The textarea height matches the original block's height (±1px).
 *
 * These tests will FAIL with the current implementation (bare <textarea>
 * replacing the block loses Bootstrap's element-type margin rules) and
 * PASS after the wrapper-element fix (keeping the <p> / <hN> element live
 * so CSS margins are preserved automatically).
 *
 * Plan 2b deferred note: "The reflow criterion requires real layout —
 * it is a Playwright test in q2-preview-spa/."  These are those tests.
 */

import { test, expect, type Page } from '@playwright/test';
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

/** Wait for the q2-preview sourceIndex to build and editable blocks to appear. */
async function waitForEditableBlocks(page: Page): Promise<void> {
    const iframe = page.frameLocator('iframe');
    await iframe.locator('[data-block-pool-id]').first().waitFor({ timeout: 15_000 });
}

let server: PreviewServerHandle;

test.beforeEach(async () => {
    server = await startPreviewServer({
        fixtureFiles: [{ path: 'index.qmd', content: FIXTURE }],
    });
});

test.afterEach(async () => {
    await server?.stop();
});

test('paragraph: wrapper height and following heading top are unchanged on activation', async ({ page }) => {
    await page.goto(server.url);
    await waitForEditableBlocks(page);

    const iframe = page.frameLocator('iframe');
    const para = iframe.locator('p[data-block-pool-id]').first();
    const heading = iframe.locator('h2').first();

    // Measure before activation.
    const paraBox = await para.boundingBox();
    expect(paraBox, 'paragraph must be visible before activation').not.toBeNull();
    const headingTopBefore = (await heading.boundingBox())?.y ?? 0;

    // Click to activate; wait for the textarea.
    await para.click();
    const textarea = iframe.locator('textarea').first();
    await textarea.waitFor({ timeout: 5_000 });

    // Measure wrapper height after activation (P1 sizing: wrapper must keep its original size).
    const paraBoxAfter = await para.boundingBox();
    expect(paraBoxAfter, 'paragraph wrapper must stay in DOM after activation').not.toBeNull();
    const headingTopAfter = (await heading.boundingBox())?.y ?? 0;

    // Wrapper (p) must not change height on activation.
    expect(
        Math.abs(paraBoxAfter!.height - paraBox!.height),
        `paragraph height changed from ${paraBox!.height}px to ${paraBoxAfter!.height}px on activation`,
    ).toBeLessThanOrEqual(1);

    // Heading must not shift (P1 no-reflow — the key assertion, fails without wrapper fix).
    expect(
        Math.abs(headingTopAfter - headingTopBefore),
        `heading top shifted by ${Math.abs(headingTopAfter - headingTopBefore).toFixed(1)}px on paragraph activation (was ${headingTopBefore.toFixed(1)}px, now ${headingTopAfter.toFixed(1)}px); margin-bottom was likely lost`,
    ).toBeLessThanOrEqual(1);
});

test('heading: wrapper height and following list top are unchanged on activation', async ({ page }) => {
    await page.goto(server.url);
    await waitForEditableBlocks(page);

    const iframe = page.frameLocator('iframe');
    const heading = iframe.locator('h2[data-block-pool-id]').first();
    const list = iframe.locator('ul').first();

    // Measure before activation.
    const headingBox = await heading.boundingBox();
    expect(headingBox, 'heading must be visible before activation').not.toBeNull();
    const listTopBefore = (await list.boundingBox())?.y ?? 0;

    // Click to activate; wait for the textarea.
    await heading.click();
    const textarea = iframe.locator('textarea').first();
    await textarea.waitFor({ timeout: 5_000 });

    // Measure wrapper height after activation (P1 sizing: wrapper must keep its original size).
    const headingBoxAfter = await heading.boundingBox();
    expect(headingBoxAfter, 'heading wrapper must stay in DOM after activation').not.toBeNull();
    const listTopAfter = (await list.boundingBox())?.y ?? 0;

    // Wrapper (h2) must not change height on activation.
    expect(
        Math.abs(headingBoxAfter!.height - headingBox!.height),
        `heading height changed from ${headingBox!.height}px to ${headingBoxAfter!.height}px on activation`,
    ).toBeLessThanOrEqual(1);

    // List must not shift (P1 no-reflow — the key assertion, fails without wrapper fix).
    expect(
        Math.abs(listTopAfter - listTopBefore),
        `list top shifted by ${Math.abs(listTopAfter - listTopBefore).toFixed(1)}px on heading activation (was ${listTopBefore.toFixed(1)}px, now ${listTopAfter.toFixed(1)}px); margin-bottom was likely lost`,
    ).toBeLessThanOrEqual(1);
});

test('paragraph above list: wrapper height and list top unchanged on activation', async ({ page }) => {
    await page.goto(server.url);
    await waitForEditableBlocks(page);

    const iframe = page.frameLocator('iframe');
    // Second paragraph — sits directly above a list with no heading between them.
    const para = iframe.locator('p[data-block-pool-id]').nth(1);
    // Second list — the direct following sibling of the second paragraph.
    const list = iframe.locator('ul').nth(1);

    // Measure before activation.
    const paraBox = await para.boundingBox();
    expect(paraBox, 'second paragraph must be visible before activation').not.toBeNull();
    const listTopBefore = (await list.boundingBox())?.y ?? 0;

    // Click to activate; wait for the textarea.
    await para.click();
    const textarea = iframe.locator('textarea').first();
    await textarea.waitFor({ timeout: 5_000 });

    // Measure wrapper height after activation.
    const paraBoxAfter = await para.boundingBox();
    expect(paraBoxAfter, 'paragraph wrapper must stay in DOM after activation').not.toBeNull();
    const listTopAfter = (await list.boundingBox())?.y ?? 0;

    // Wrapper (p) must not change height on activation.
    expect(
        Math.abs(paraBoxAfter!.height - paraBox!.height),
        `paragraph height changed from ${paraBox!.height}px to ${paraBoxAfter!.height}px on activation`,
    ).toBeLessThanOrEqual(1);

    // List must not shift (para→list adjacency, no heading between them).
    expect(
        Math.abs(listTopAfter - listTopBefore),
        `list top shifted by ${Math.abs(listTopAfter - listTopBefore).toFixed(1)}px on paragraph activation (was ${listTopBefore.toFixed(1)}px, now ${listTopAfter.toFixed(1)}px); margin-bottom was likely lost`,
    ).toBeLessThanOrEqual(1);
});
