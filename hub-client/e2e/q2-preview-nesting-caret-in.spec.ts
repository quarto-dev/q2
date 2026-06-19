/**
 * §2 caret-aware nest-in acceptance — real browser.
 *
 * The feature: nest-IN descends toward the surface the LIVE CARET is in, not the
 * frozen `leafAnchorR0` ("first clicked leaf"). Editing the whole list and moving
 * the caret onto a deeper item's line, then nest-in, must open the editor on the
 * caret-toward child — NOT the first item.
 *
 * Interaction: open the first item ("another") → nest-OUT to the whole list →
 * move the caret onto the "nother" line (source line 4) → nest-IN. The caret line
 * descends into the level-1 sub-list ([20,69]); the editor's buffer therefore
 * contains "sub-item" (the sub-list's content), not the bare first item "another".
 *
 * Real browser because the caret read depends on a real textarea selection that
 * survives the breadcrumb ▶ button's preventDefault; the which-surface wiring is
 * also covered at the jsdom tier (nest-caret.integration.test.tsx).
 *
 * Fail-on-revert: disable the centralized caret read (fall back to leafAnchorR0,
 * which points at "another") → nest-in opens the first item → the buffer is
 * "another" (no "sub-item") → the assertion goes RED.
 *
 * Run via:
 *   cd hub-client && VITE_E2E=1 npm run build
 *   npx playwright test e2e/q2-preview-nesting-caret-in.spec.ts --project=chromium --workers=1
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

const QMD = [
    '---',
    'format: q2-preview',
    '---',
    '',
    '* another',
    '* hello',
    '    * sub-item',
    '        * sub-sub-item',
    '    * nother',
    '',
].join('\n');

test.describe('§2 — caret-aware nest-in (real browser)', () => {
    test.setTimeout(120000);

    test.beforeEach(async ({ page }, testInfo) => {
        if (testInfo.workerIndex > 0) await page.waitForTimeout(1000);
    });

    test('nest-in follows the caret line into the sub-list, not the frozen first item', async ({ page }) => {
        await page.addInitScript(() => {
            localStorage.setItem('quarto-hub:preferences', JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
                unlockNestingCursor: true,
            }));
        });

        const serverUrl = getServerUrl();
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'nesting-caret-in.qmd', content: QMD, contentType: 'text' },
        ]);
        const iframe = await openFile(page, serverUrl, docId, 'nesting-caret-in.qmd');

        // Open the FIRST item ("another"), seeding leafAnchorR0 INTO "another".
        await iframe.getByText('another', { exact: true }).click();
        await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });

        // Nest OUT to the whole list → the editor now holds the full list text.
        await iframe.getByRole('button', { name: /^Out/ }).click();
        await iframe.locator('#q2-active-edit-region').waitFor({ timeout: 10_000 });
        const wholeListValue = await iframe.locator('textarea').first().inputValue();
        expect(wholeListValue).toContain('sub-item'); // we really are on the whole list

        // Move the caret onto the "nother" line (source line 4). Use "* nother"
        // (NOT "nother", which is a substring of "a-nother" on line 0).
        await iframe.locator('textarea').first().evaluate((el) => {
            const ta = el as HTMLTextAreaElement;
            const idx = ta.value.indexOf('* nother');
            ta.focus();
            ta.selectionStart = ta.selectionEnd = idx;
        });

        // Nest IN → must descend toward the CARET line (the level-1 sub-list),
        // NOT the leafAnchorR0 first item ("another").
        await iframe.getByRole('button', { name: /^In/ }).click();
        await iframe.locator('#q2-active-edit-region').waitFor({ timeout: 10_000 });

        const inValue = await iframe.locator('textarea').first().inputValue();
        console.log(`nest-in opened editor buffer: ${JSON.stringify(inValue)}`);

        // The caret-toward child is the sub-list → its buffer contains "sub-item".
        // The pre-fix / fail-on-revert behaviour opens the first item "another".
        expect(inValue, 'nest-in must open the caret-toward sub-list (contains "sub-item")').toContain('sub-item');
        expect(inValue.trim(), 'nest-in must NOT open the frozen first item "another"').not.toBe('another');

        await iframe.locator('textarea').first().press('Escape');
    });
});
