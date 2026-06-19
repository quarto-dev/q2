/**
 * §0.d — tight-list ITEM activation sizes the editor to the ITEM, not the list
 * (real browser).
 *
 * The originally-reported bug §0 fixes: in UNLOCK mode, clicking a tight single-
 * block bullet item's text used to activate (and size the editor to) the WHOLE
 * `<ul>`. §0 makes `<li>`/`<dd>` borrow their leading `Plain` block's pool-id, so
 * a click on an item activates the ITEM (its leading Plain) and the editor sizes
 * to that ITEM's box (≈ one line), strictly less than the whole-list height.
 *
 * Measured geometry (real browser): a 3-item bullet list renders a `<ul>` ≈76.5px
 * tall; each `<li>` leading line is ≈25.5px. Clicking "one" opens an editor whose
 * buffer is exactly "one" (the item, not the list) and whose active-region height
 * ≈ the item line, well under the list height.
 *
 * Fail-on-revert: drop the §0 `<li>` pool-id borrow so a tight-item click resolves
 * to the whole `<ul>` → the editor buffer becomes the whole list AND the edit
 * height jumps to ≈ the list height → both the "buffer == 'one'" and the
 * "editH < listH" assertions go RED.
 *
 * Run via:
 *   cd hub-client && npx playwright test e2e/q2-preview-item-edit-size.spec.ts --project=chromium --workers=1
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

const QMD = ['---', 'format: q2-preview', '---', '', '- one', '- two', '- three', ''].join('\n');

const TOL = 2;

test.describe('§0.d — tight-list item activation sizes to the item (real browser)', () => {
    test.setTimeout(120000);

    test.beforeEach(async ({ page }, testInfo) => {
        if (testInfo.workerIndex > 0) await page.waitForTimeout(1000);
    });

    test('clicking a tight bullet item opens the ITEM editor (≈ one line, < the whole-list height)', async ({ page }) => {
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
            { path: 'item-edit-size.qmd', content: QMD, contentType: 'text' },
        ]);
        const iframe = await openFile(page, serverUrl, docId, 'item-edit-size.qmd');

        // Original geometry: the whole list and a single item's leading line.
        const listH = (await iframe.locator('ul[data-block-pool-id]').first().boundingBox())!.height;
        const itemH = (await iframe.locator('li', { hasText: 'one' }).first().boundingBox())!.height;
        console.log(`list height=${listH.toFixed(1)}  item height=${itemH.toFixed(1)}`);
        // Sanity: the fixture really has a multi-item list (item ≪ list), otherwise
        // the "sized to item, not list" distinction below would be vacuous.
        expect(itemH).toBeLessThan(listH - TOL);

        // Click the tight item "one". §0: this activates the ITEM proxy, not the list.
        await iframe.getByText('one', { exact: true }).click();
        await iframe.locator('#q2-active-edit-region').waitFor({ timeout: 10_000 });

        const buffer = await iframe.locator('textarea').first().inputValue();
        const editH = (await iframe.locator('#q2-active-edit-region').boundingBox())!.height;
        console.log(`item editor: buffer=${JSON.stringify(buffer)}  editH=${editH.toFixed(1)}`);

        // The editor holds JUST the item ("one"), not the whole list — proves §0
        // item activation (the pre-fix bug opened the whole `<ul>` source).
        expect(
            buffer.trim(),
            `clicking a tight item must open the ITEM editor (buffer "one"), not the whole list`,
        ).toBe('one');

        // The editor sizes to the ITEM box (≈ one line), strictly less than the
        // whole-list height (the originally-reported bug §0 fixes).
        expect(
            Math.abs(editH - itemH),
            `item editor height (${editH.toFixed(1)}) must ≈ the item line height (${itemH.toFixed(1)})`,
        ).toBeLessThanOrEqual(TOL);
        expect(
            editH,
            `item editor height (${editH.toFixed(1)}) must be < the whole-list height (${listH.toFixed(1)}) — the pre-§0 bug sized it to the list`,
        ).toBeLessThan(listH - TOL);

        await iframe.locator('textarea').first().press('Escape');
    });
});
