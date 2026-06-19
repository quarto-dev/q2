/**
 * T13(c) Crumb-Does-Not-Carry-Expansion: a crumb-jump to an ancestor does NOT
 * carry the current editor's expanded state into the destination editor.
 *
 * The production behavior: `executeLanding` uses `carryExpanded = pl.spec.kind === 'nest'`.
 * Crumb jumps have `kind: 'crumb'`, so `carryExpanded = false`, so `openEditTarget`
 * receives `keepExpanded: false` and resets `editExpandedRef.current = false`.
 * The relanded editor opens collapsed.
 *
 * This is a real-browser test because:
 * - The breadcrumb chip renders only under real browser layout (useLayoutEffect reads
 *   getBoundingClientRect(); jsdom returns zero rects → chip renders at wrong position
 *   or not at all).
 * - The crumb button click requires a real non-zero-size DOM element to be reachable.
 * - jsdom tests of crumb-click behavior went VACUOUS (the chip didn't render, the
 *   click did nothing, the test passed for the wrong reason).
 *
 * Fail-on-revert (primary): in PreviewRoot.tsx ~line 784, change:
 *   `const carryExpanded = pl.spec.kind === 'nest';`
 * to:
 *   `const carryExpanded = true; // always carry`
 * → `executeLanding` passes `keepExpanded: true` for crumb landings → the relanded
 * editor inherits `editExpandedRef.current = true` → `data-expanded` is present
 * immediately → ASSERTION A (`expandedAfter === false`) flips RED.
 *
 * Fail-on-revert (alternate): in PreviewRoot.tsx ~line 513, remove:
 *   `if (!opts.keepExpanded) editExpandedRef.current = false;`
 * → `editExpandedRef` always carries → same RED result.
 *
 * Run via:
 *   cd hub-client && VITE_E2E=1 npm run build
 *   npx playwright test e2e/q2-preview-crumb-no-carry-expansion.spec.ts --project=chromium --workers=1
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

// Fixture: a fenced Div (depth-1 ancestor) wrapping a multi-line Para.
// The multi-line Para expands visibly when edited (source height > render height).
const QMD = [
    '---',
    'format: q2-preview',
    '---',
    '',
    '::: {.outer}',
    '',
    'Inner paragraph with several lines of text',
    'that span more than one source line',
    'and will expand when the editor opens.',
    '',
    ':::',
    '',
].join('\n');

test.describe('T13(c) — Crumb jump does NOT carry expansion into destination editor (real browser)', () => {
    test.setTimeout(120000);

    test.beforeEach(async ({ page }, testInfo) => {
        if (testInfo.workerIndex > 0) await page.waitForTimeout(1000);
    });

    test('crumb-jump to ancestor relands collapsed (data-expanded absent), not expanded', async ({ page }) => {
        // 1. Enable unlock mode before any navigation.
        await page.addInitScript(() => {
            localStorage.setItem('quarto-hub:preferences', JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
                unlockNestingCursor: true,
            }));
        });

        // 2. Create project and open file.
        const serverUrl = getServerUrl();
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'crumb-no-carry.qmd', content: QMD, contentType: 'text' },
        ]);
        const iframe = await openFile(page, serverUrl, docId, 'crumb-no-carry.qmd');

        // 3. Open the inner Para editor.
        await iframe.locator('p[data-block-pool-id]').first().click();
        await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });

        // 4. Verify the breadcrumb chip is visible (we are in a nested surface).
        const chip = iframe.locator('[data-testid="q2-breadcrumb-chip"]');
        await chip.waitFor({ timeout: 5_000 });

        // 5. Trigger EXPANSION: press 'x' (the §7 expand-on-edit trigger).
        await iframe.locator('textarea').first().press('x');
        // Wait for data-expanded to appear on the textarea.
        await expect(iframe.locator('textarea[data-expanded]')).toBeVisible({ timeout: 3_000 });

        // Confirm expanded before the crumb click.
        const expandedBefore = await iframe.locator('textarea').first().evaluate(
            (el) => (el as HTMLTextAreaElement).hasAttribute('data-expanded'),
        );
        expect(expandedBefore, 'editor must be expanded before crumb click').toBe(true);

        // Record the expanded height for the soft assertion (ASSERTION C).
        const expandedHeight = await iframe.locator('textarea').first().evaluate(
            (el) => (el as HTMLTextAreaElement).getBoundingClientRect().height,
        );

        // 6. Log crumb titles for empirical verification of the Div crumb label.
        // Per the design doc: "validate the Div crumb title empirically".
        const crumbTitles = await chip.locator('.q2-crumb').evaluateAll(
            (els) => els.map((el) => el.getAttribute('title') ?? ''),
        );
        console.log(`crumb-no-carry: crumb titles: ${JSON.stringify(crumbTitles)}`);

        // 7. Click the OUTERMOST ancestor crumb (the Div crumb — title="Div" per
        // buildAncestorPath in BreadcrumbChip.tsx).
        // We try getByRole({ name: 'Div' }) first (per design doc); fall back to
        // .q2-crumb.first() if no crumb with that name is found.
        const divCrumbByRole = chip.getByRole('button', { name: 'Div' });
        const divCrumbByRoleCount = await divCrumbByRole.count();
        console.log(`crumb-no-carry: crumb with name='Div' count=${divCrumbByRoleCount}`);

        if (divCrumbByRoleCount > 0) {
            await divCrumbByRole.click();
        } else {
            // Fallback: click the outermost (first) .q2-crumb — the Div ancestor.
            console.log('crumb-no-carry: falling back to first .q2-crumb (Div name not matched)');
            await chip.locator('.q2-crumb').first().click();
        }

        // 8. Wait for the reland on the Div surface.
        await iframe.locator('#q2-active-edit-region').waitFor({ timeout: 10_000 });
        await iframe.locator('textarea').first().waitFor({ timeout: 5_000 });

        // ASSERTION A: the relanded editor is NOT expanded.
        // data-expanded must be ABSENT — crumb jumps do not carry expansion.
        // Fail-on-revert: set carryExpanded=true at PreviewRoot.tsx ~line 784 →
        // editExpandedRef carries true → data-expanded present → RED.
        const expandedAfter = await iframe.locator('textarea').first().evaluate(
            (el) => (el as HTMLTextAreaElement).hasAttribute('data-expanded'),
        );
        expect(
            expandedAfter,
            'crumb-jumped editor must NOT be expanded (crumb jumps reset expansion)',
        ).toBe(false);

        // ASSERTION B: the relanded editor IS on the Div surface (buffer contains the Para text).
        const landedBuffer = await iframe.locator('textarea').first().inputValue();
        expect(
            landedBuffer,
            'crumb-jumped editor must contain the inner Para text (landed on the Div)',
        ).toContain('Inner paragraph');

        // ASSERTION C: soft height check (log only — real gate is ASSERTION A).
        const collapsedHeight = await iframe.locator('textarea').first().evaluate(
            (el) => (el as HTMLTextAreaElement).getBoundingClientRect().height,
        );
        console.log(
            `crumb-no-carry-expansion: expandedHeight=${expandedHeight.toFixed(1)}, collapsedHeight=${collapsedHeight.toFixed(1)}`,
        );
        // The Div surface may be taller or shorter than the Para surface.
        // The real gate is data-expanded absent (ASSERTION A), not height.
        // Log for human review; do not fail on height alone.

        await iframe.locator('textarea').first().press('Escape');
    });
});
