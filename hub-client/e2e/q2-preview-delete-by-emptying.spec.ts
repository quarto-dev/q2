/**
 * §6.g — delete-by-emptying a paragraph removes it from the DOM AND the Automerge
 * source (real browser).
 *
 * §6: emptying a block's text and committing (Cmd/Ctrl+Enter, arrow-away, or blur)
 * DELETES the block (commits empty text → backend removes it). This spec drives the
 * Cmd/Ctrl+Enter path end-to-end: select a paragraph, clear its text, commit, and
 * verify the paragraph is gone from both the rendered iframe DOM and the underlying
 * QMD (via the Automerge VFS hook), while a sibling paragraph survives.
 *
 * Fail-on-revert: revert §6's delete branch (treat empty draft as cancel) → the
 * paragraph survives commit → `lacks: ['First paragraph.']` goes RED (the text is
 * still present in the source) and the DOM still shows it.
 *
 * Run via:
 *   cd hub-client && npx playwright test e2e/q2-preview-delete-by-emptying.spec.ts --project=chromium --workers=1
 */

import { test, expect, type Page, type FrameLocator } from '@playwright/test';
import type {} from './helpers/testHooks';
import {
    bootstrapProjectSet,
    createProjectOnServer,
    seedProjectInBrowser,
    getServerUrl,
} from './helpers/projectFactory';
import { waitForPreviewRender } from './helpers/previewExtraction';
import { assertAutomerge } from './helpers/assertAutomerge';

async function openFile(
    page: Page,
    serverUrl: string,
    docId: string,
    filename: string,
): Promise<FrameLocator> {
    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, docId, serverUrl);
    await page.goto(`/#/p/${localId}/file/${filename}`);
    await waitForPreviewRender(page, { kind: 'q2-preview', timeout: 30000 });
    const iframe = page.frameLocator('iframe[src*="q2-preview.html"]');
    await iframe.locator('[data-block-pool-id]').first().waitFor({ timeout: 15_000 });
    return iframe;
}

const QMD =
    '---\nformat: q2-preview\n---\n\nFirst paragraph.\n\nSecond paragraph.\n';

test.describe('§6.g — delete a paragraph by emptying + commit (real browser)', () => {
    test.setTimeout(120000);

    test.beforeEach(async ({ page }, testInfo) => {
        if (testInfo.workerIndex > 0) await page.waitForTimeout(1000);
    });

    test('clearing a paragraph and pressing Ctrl+Enter removes it from the DOM and Automerge', async ({ page }) => {
        const serverUrl = getServerUrl();
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'delete-by-emptying.qmd', content: QMD, contentType: 'text' },
        ]);
        const iframe = await openFile(page, serverUrl, docId, 'delete-by-emptying.qmd');
        await expect(iframe.locator('text=First paragraph.')).toBeVisible();

        // Activate the FIRST paragraph and clear it (empty draft from non-empty
        // baseline → §6 delete signal).
        await iframe.locator('p[data-block-pool-id]').first().click();
        const ta = iframe.locator('textarea').first();
        await ta.waitFor({ timeout: 10_000 });
        await ta.fill('');
        await ta.press('Control+Enter');

        // The paragraph is removed from the rendered DOM (the sibling remains).
        await expect(iframe.locator('text=First paragraph.')).toHaveCount(0, { timeout: 10_000 });
        await expect(iframe.locator('text=Second paragraph.')).toBeVisible();

        // And it is removed from the Automerge source (the sibling text survives).
        await assertAutomerge(page, 'delete-by-emptying.qmd', {
            lacks: ['First paragraph.'],
            contains: ['Second paragraph.'],
        });
    });
});
