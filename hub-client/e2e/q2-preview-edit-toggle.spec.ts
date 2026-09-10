/**
 * Edit pill in the document bottom bar (bd-ew0vak6b) — real browser.
 *
 * `format: q2-preview` used to be always editable: a mouse click on any
 * editable paragraph opens that paragraph's editor, so a click on a link
 * inside a paragraph also opened an editor. The Edit pill in the replay bar
 * turns block editing off, which makes the preview behave like a read-only
 * `q2 preview` (no `--allow-edit`): no edit affordances, no hover outline,
 * and links are plain links. The choice persists as the `previewEditing`
 * preference.
 *
 * Coverage (one document with a link to a second document):
 *   1. default: editing ON — every top-level block carries a pool-id;
 *   2. click the pill → OFF: no pool-ids in the iframe, hovering a paragraph
 *      adds no outline, and clicking the link navigates to the second
 *      document without opening an editor;
 *   3. reload → still OFF (persisted), pill reads "Editing off";
 *   4. click the pill again → ON: affordances return.
 *
 * Run via:
 *   cd hub-client && VITE_E2E=1 npm run build
 *   npx playwright test e2e/q2-preview-edit-toggle.spec.ts --project=chromium --workers=1
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

const IFRAME_SELECTOR = 'iframe[src*="q2-preview.html"]';

async function openProjectFile(
    page: Page,
    serverUrl: string,
    docId: string,
    filename: string,
): Promise<FrameLocator> {
    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, docId, serverUrl);
    await page.goto(`/#/p/${localId}/file/${filename}`);
    await waitForPreviewRender(page, { kind: 'q2-preview', timeout: 30000 });
    return page.frameLocator(IFRAME_SELECTOR);
}

const MAIN_QMD = [
    '---',
    'format: q2-preview',
    '---',
    '',
    'First paragraph with a [link to the other page](other.qmd) inside it.',
    '',
    'Second paragraph, plain text.',
    '',
].join('\n');

const OTHER_QMD = [
    '---',
    'format: q2-preview',
    '---',
    '',
    'The other page.',
    '',
].join('\n');

const pill = (page: Page) => page.getByRole('button', { name: /^Editing (on|off)$/ });

test.describe('Edit pill — q2-preview editable / read-only toggle (real browser)', () => {
    test.setTimeout(150000);

    test.beforeEach(async ({ page }, testInfo) => {
        if (testInfo.workerIndex > 0) await page.waitForTimeout(1000);
    });

    test('turns edit affordances off, makes links plain, persists across reload, and turns back on', async ({ page }) => {
        const serverUrl = getServerUrl();
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'main.qmd', content: MAIN_QMD, contentType: 'text' },
            { path: 'other.qmd', content: OTHER_QMD, contentType: 'text' },
        ]);
        let iframe = await openProjectFile(page, serverUrl, docId, 'main.qmd');

        // 1. Default: editing ON. Both paragraphs advertise editability and
        //    the pill reads "on".
        await iframe.locator('p[data-block-pool-id]').first().waitFor({ timeout: 15_000 });
        expect(await iframe.locator('p[data-block-pool-id]').count()).toBe(2);
        await expect(pill(page)).toHaveAttribute('aria-pressed', 'true');
        await expect(pill(page)).toHaveAccessibleName('Editing on');

        // 2. Click the pill → OFF. The iframe re-renders with no affordance
        //    at all (this is the same `editingDisabled` path as a read-only
        //    `q2 preview`).
        await pill(page).click();
        await expect(pill(page)).toHaveAttribute('aria-pressed', 'false');
        await expect(iframe.locator('[data-block-pool-id]')).toHaveCount(0, { timeout: 10_000 });

        // Hovering a paragraph adds no outline (the hover surface is inert).
        await iframe.getByText('Second paragraph', { exact: false }).hover();
        const outlined = await iframe.locator('p').evaluateAll((els) =>
            (els as HTMLElement[]).filter((e) => e.style.boxShadow !== '').length,
        );
        expect(outlined, 'no paragraph is outlined on hover while editing is off').toBe(0);

        // Clicking the link navigates to the other document — and opens no
        // editor along the way (with editing on, the same click would also
        // open the paragraph's editor).
        await iframe.getByRole('link', { name: 'link to the other page' }).click();
        await page.waitForURL(/\/file\/other\.qmd/, { timeout: 15_000 });
        await waitForPreviewRender(page, { kind: 'q2-preview', timeout: 30000 });
        iframe = page.frameLocator(IFRAME_SELECTOR);
        await iframe.getByText('The other page.').waitFor({ timeout: 15_000 });
        expect(await iframe.locator('textarea').count(), 'no plain editor opened').toBe(0);
        expect(await iframe.locator('.q2-rt-toolbar').count(), 'no rich editor opened').toBe(0);
        expect(await iframe.locator('[data-block-pool-id]').count(), 'still read-only on the other page').toBe(0);

        // 3. Reload: the preference persisted, so the preview comes back
        //    read-only and the pill still reads "off".
        await page.reload();
        await waitForPreviewRender(page, { kind: 'q2-preview', timeout: 30000 });
        iframe = page.frameLocator(IFRAME_SELECTOR);
        await iframe.getByText('The other page.').waitFor({ timeout: 15_000 });
        await expect(pill(page)).toHaveAttribute('aria-pressed', 'false');
        await expect(pill(page)).toHaveAccessibleName('Editing off');
        expect(await iframe.locator('[data-block-pool-id]').count()).toBe(0);

        // 4. Click the pill again → ON: the affordance returns.
        await pill(page).click();
        await expect(pill(page)).toHaveAttribute('aria-pressed', 'true');
        await expect(iframe.locator('p[data-block-pool-id]')).toHaveCount(1, { timeout: 10_000 });
    });
});
