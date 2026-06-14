/**
 * E2E tests for Plan 2b P1: edit surface no-reflow (sizing + spacing).
 *
 * Asserts both parts of the no-reflow contract (active-region sizing + the
 * following sibling not moving) via the shared, host-agnostic
 * `assertNoReflowOnActivation` from `@quarto/preview-e2e-helpers` — the same
 * helper the hub-client e2e uses, so the two hosts exercise identical assertion
 * logic against the same `@quarto/preview-renderer`. Here the host is the real
 * `q2 preview` binary (via startPreviewServer); the helper takes the iframe
 * FrameLocator and stays unaware of how the document was opened.
 *
 * See the helper module for the contract details (the active editor is REPLACED
 * by a `<div id="q2-active-edit-region">`, so the "after" box is measured there,
 * never by re-querying `<tag>[data-block-pool-id]`).
 */

import { test, type Page } from '@playwright/test';
import { assertNoReflowOnActivation } from '@quarto/preview-e2e-helpers';
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
        iframe,
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
        iframe,
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
        iframe,
        iframe.locator('p[data-block-pool-id]').nth(1),
        iframe.locator('ul').nth(1),
        'paragraph→list',
    );
});
