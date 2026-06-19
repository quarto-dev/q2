/**
 * §1 geometry-snapshot acceptance — nesting cursor SIZE-IN (real browser).
 *
 * The reported bug: a nest-IN to a child surface kept the STALE parent box (the
 * child isn't rendered while the parent's subtree is a textarea), so the editing
 * surface stayed parent-sized. §1 fixes this by sizing the destination from the
 * geometry SNAPSHOT captured at activation, where every surface's original box was
 * measured.
 *
 * Interaction: click a tight sub-list item ("nother"). Post-§0 this activates the
 * ITEM proxy (its borrowed leading-Plain box ≈ one line), NOT the whole list. We
 * then nest-OUT TWICE to climb item → sub-list → whole list (full height), then
 * nest-IN back to the sub-list. The nest-in destination must size to the sub-list's
 * ORIGINAL height (the child's snapshot box), not the whole-list box the nest-out
 * step left behind.
 *
 * §0 premise note: pre-§0, clicking "nother" seeded a leaf into the sub-list and a
 * SINGLE nest-out reached the whole list. Post-§0, clicking "nother" activates the
 * item proxy, so one nest-out only reaches the sub-list — hence the extra nest-out
 * here to still exercise the whole-list → sub-list size-in. The geometry sequence
 * (measured in a real browser) is: item 25.5 → sub-list 76.5 → whole-list 127.5,
 * and back down 127.5 → 76.5 → 25.5.
 *
 * Pixel geometry → real browser. Capture/consume wiring + key arithmetic are
 * covered at the jsdom tier; this spec proves the real render path.
 *
 * Fail-on-revert: restore the stale-box fallback (keep the parent box on nest-in)
 * → the editor stays at the whole-list height → the ≈ H1 assertion goes RED.
 *
 * Run via:
 *   cd hub-client && npm run test:e2e -- e2e/q2-preview-nesting-size-in.spec.ts --project=chromium
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

const TOL = 2;

test.describe('§1 — nesting-cursor SIZE-IN geometry (real browser)', () => {
    test.setTimeout(120000);

    test.beforeEach(async ({ page }, testInfo) => {
        if (testInfo.workerIndex > 0) await page.waitForTimeout(1000);
    });

    test('nest-in from the whole list sizes the editor to the CHILD\'s height, not the parent\'s full height', async ({ page }) => {
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
            { path: 'nesting-size-in.qmd', content: QMD, contentType: 'text' },
        ]);
        const iframe = await openFile(page, serverUrl, docId, 'nesting-size-in.qmd');

        // Original geometry: ul0 = whole list, ul1 = sub-list (the nest-in destination).
        const ul0 = iframe.locator('ul[data-block-pool-id]').nth(0);
        const ul1 = iframe.locator('ul[data-block-pool-id]').nth(1);
        const h0 = (await ul0.boundingBox())!.height; // whole list (tall)
        const h1 = (await ul1.boundingBox())!.height; // sub-list (shorter)
        console.log(`original heights: whole-list H0=${h0.toFixed(1)}  sub-list H1=${h1.toFixed(1)}`);
        expect(h0).toBeGreaterThan(h1 + TOL);

        // Click the tight sub-list item "nother". Post-§0 this activates the ITEM
        // proxy (borrowed leading-Plain box ≈ one line), NOT the whole list — so a
        // single nest-out only reaches the sub-list. Sanity-check the item box is
        // ≈ one line (well under the sub-list height), proving §0 item activation.
        await iframe.getByText('nother', { exact: true }).click();
        await iframe.locator('#q2-active-edit-region').waitFor({ timeout: 10_000 });
        const itemBox = await iframe.locator('#q2-active-edit-region').boundingBox();
        console.log(`item-proxy edit height after click: ${itemBox!.height.toFixed(1)} (expect ≪ H1=${h1.toFixed(1)})`);
        expect(
            itemBox!.height,
            `clicking the tight item "nother" must activate the ITEM proxy (≈ one line), not the sub-list (${h1.toFixed(1)}px)`,
        ).toBeLessThan(h1 - TOL);

        // Nest OUT once → climbs item → sub-list (≈ H1).
        await iframe.getByRole('button', { name: /^Out/ }).click();
        await iframe.locator('#q2-active-edit-region').waitFor({ timeout: 10_000 });
        // Nest OUT again → climbs sub-list → whole list (≈ H0).
        await iframe.getByRole('button', { name: /^Out/ }).click();
        await iframe.locator('#q2-active-edit-region').waitFor({ timeout: 10_000 });
        const outBox = await iframe.locator('#q2-active-edit-region').boundingBox();
        expect(Math.abs(outBox!.height - h0)).toBeLessThanOrEqual(TOL);

        // Nest IN back to the sub-list (leafAnchorR0 preserved) → must size to the
        // sub-list's ORIGINAL height from the snapshot, not the whole-list box.
        await iframe.getByRole('button', { name: /^In/ }).click();
        await iframe.locator('#q2-active-edit-region').waitFor({ timeout: 10_000 });

        const editBox = await iframe.locator('#q2-active-edit-region').boundingBox();
        expect(editBox, 'active edit region must have a bounding box').not.toBeNull();
        const he = editBox!.height;
        console.log(`edit-region height after nest-in: ${he.toFixed(1)} (expect ≈ H1=${h1.toFixed(1)}, not ≈ H0=${h0.toFixed(1)})`);

        // The editing surface now covers the CHILD sub-list → height ≈ H1, NOT the
        // parent's full height H0 (the pre-fix stale-box bug).
        expect(
            Math.abs(he - h1),
            `nest-in edit height (${he.toFixed(1)}) must ≈ the sub-list original height H1 (${h1.toFixed(1)})`,
        ).toBeLessThanOrEqual(TOL);
        expect(
            he,
            `nest-in edit height (${he.toFixed(1)}) must be < the whole-list height H0 (${h0.toFixed(1)}) — the pre-fix bug kept it at ~H0`,
        ).toBeLessThan(h0 - TOL);

        await iframe.locator('textarea').first().press('Escape');
    });
});
