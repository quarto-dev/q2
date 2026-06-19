/**
 * §7.e — expand-on-edit: a click-activated editor opens COLLAPSED (render height,
 * no `data-expanded`) and GROWS on the first in-surface keystroke (real browser).
 *
 * §7 gives the edit textarea a third "expanded" size state. It opens collapsed
 * (height == the replaced element's render height) and grows to fit the full
 * source text on the first IN-SURFACE printable keystroke. The seam is the
 * `data-expanded` attribute on the `<textarea>` (present ↔ expanded).
 *
 * This fixture authors a paragraph across THREE source lines that wrap to fewer
 * visual lines, so the SOURCE is taller than the RENDER — the case where collapsed
 * vs expanded actually differ. Measured (real browser): click-open height ≈51px
 * (render), `data-expanded` absent; after typing one char the textarea gains
 * `data-expanded`, its clientHeight grows (≈49→71) and its box grows (≈51→73) to
 * fit the 3 source lines. Deleting the added char leaves the editor expanded at the
 * full source height — never shrinking below the collapsed open height.
 *
 * Fail-on-revert:
 *  - Remove the `data-expanded` attribute → the "present after typing" assert RED.
 *  - Remove the expand trigger in onKeyDown → after typing, `data-expanded` stays
 *    absent AND the height does not grow → both grow-asserts RED.
 *  - Open already-expanded (drop the collapsed-open state) → the open-time
 *    "`data-expanded` absent" assert RED.
 *
 * Run via:
 *   cd hub-client && npx playwright test e2e/q2-preview-expand-on-edit.spec.ts --project=chromium --workers=1
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

// A paragraph whose SOURCE spans three lines but renders to fewer visual lines,
// so the collapsed (render) height is strictly less than the expanded (source)
// height — the regime where the expand-on-edit state is observable.
const QMD = [
    '---',
    'format: q2-preview',
    '---',
    '',
    'This sentence is authored',
    'across three source lines',
    'that all flow together.',
    '',
    'Sibling para.',
    '',
].join('\n');

/** Read the active textarea's expansion flag and heights. */
async function readTextarea(iframe: FrameLocator) {
    return iframe.locator('textarea').first().evaluate((el) => {
        const ta = el as HTMLTextAreaElement;
        return {
            expanded: ta.hasAttribute('data-expanded'),
            clientHeight: ta.clientHeight,
            boxHeight: ta.getBoundingClientRect().height,
        };
    });
}

test.describe('§7.e — expand-on-edit collapsed→expanded on first keystroke (real browser)', () => {
    test.setTimeout(120000);

    test.beforeEach(async ({ page }, testInfo) => {
        if (testInfo.workerIndex > 0) await page.waitForTimeout(1000);
    });

    test('click opens collapsed (no data-expanded); typing a char expands + grows; deleting it stays ≥ open', async ({ page }) => {
        const serverUrl = getServerUrl();
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'expand-on-edit.qmd', content: QMD, contentType: 'text' },
        ]);
        const iframe = await openFile(page, serverUrl, docId, 'expand-on-edit.qmd');
        await expect(iframe.locator('text=that all flow together.')).toBeVisible();

        // CLICK-activate the paragraph → opens COLLAPSED (no data-expanded).
        await iframe.locator('p[data-block-pool-id]').first().click();
        const ta = iframe.locator('textarea').first();
        await ta.waitFor({ timeout: 10_000 });

        const open = await readTextarea(iframe);
        console.log(`open: expanded=${open.expanded} client=${open.clientHeight} box=${open.boxHeight.toFixed(1)}`);
        expect(open.expanded, 'click activation must open COLLAPSED (no data-expanded)').toBe(false);

        // Type one printable char IN-SURFACE → must expand and grow.
        await ta.focus();
        await ta.press('x');
        await page.waitForTimeout(250);

        const typed = await readTextarea(iframe);
        console.log(`after-type: expanded=${typed.expanded} client=${typed.clientHeight} box=${typed.boxHeight.toFixed(1)}`);
        expect(typed.expanded, 'data-expanded must be present after the first in-surface keystroke').toBe(true);
        expect(
            typed.clientHeight,
            `clientHeight must grow on expand (was ${open.clientHeight}, now ${typed.clientHeight})`,
        ).toBeGreaterThan(open.clientHeight);
        expect(
            typed.boxHeight,
            `textarea box must grow on expand (was ${open.boxHeight.toFixed(1)}, now ${typed.boxHeight.toFixed(1)})`,
        ).toBeGreaterThan(open.boxHeight + 1);

        // Delete the added char → editor may shrink but never below the open height.
        await ta.press('Backspace');
        await page.waitForTimeout(250);
        const deleted = await readTextarea(iframe);
        console.log(`after-delete: expanded=${deleted.expanded} client=${deleted.clientHeight} box=${deleted.boxHeight.toFixed(1)}`);
        expect(
            deleted.boxHeight,
            `after deleting the added char the editor must not shrink below the open height (${open.boxHeight.toFixed(1)})`,
        ).toBeGreaterThanOrEqual(open.boxHeight - 1);

        await ta.press('Escape');
    });
});
