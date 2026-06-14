/**
 * P3.5 tier (i) item 1 — Breadcrumb geometry (real browser, layout engine required)
 *
 * Verifies that the depth-cursor breadcrumb chip (`BreadcrumbChip.tsx`,
 * `data-testid="q2-breadcrumb-chip"`) is positioned ABOVE the active edit
 * surface — its bottom edge is anchored to the surface's top (the `- chipH`
 * term in `useLayoutEffect`) so it never occludes line 1 of the edit text.
 * At the document top it sits in the page's top margin (chipBox.y >= 0).
 *
 * jsdom returns zero rects for everything, so this geometry check is
 * Playwright-only. Production code (BreadcrumbChip.tsx) was shipped in
 * commit 56eb2d3a; this spec is the real-browser verification pass.
 *
 * Run via:
 *   cd hub-client && npx playwright test e2e/q2-preview-breadcrumb-geometry.spec.ts --project=chromium
 *
 * Prerequisites: VITE_E2E=1 npm run build (once); hub-client build in dist/.
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

// ---------------------------------------------------------------------------
// Shared setup helpers (mirrors q2-preview-block-nav-p2-5b.spec.ts)
// ---------------------------------------------------------------------------

/** Set up the project, navigate, and wait for preview + edit affordances. */
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

// ---------------------------------------------------------------------------
// Test suite
// ---------------------------------------------------------------------------

test.describe('P3.5 — Breadcrumb chip geometry (real browser)', () => {
    test.setTimeout(120000);

    // Stagger worker starts to avoid AMD init races.
    test.beforeEach(async ({ page }, testInfo) => {
        if (testInfo.workerIndex > 0) {
            await page.waitForTimeout(1000);
        }
    });

    test('chip sits above the active edit surface, never occluding line 1', async ({ page }) => {
        // Enable the depth-cursor BEFORE any navigation — addInitScript re-applies
        // on every document load, including the iframe.
        await page.addInitScript(() => {
            localStorage.setItem('quarto-hub:preferences', JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
                unlockDepthCursor: true,
            }));
        });

        const serverUrl = getServerUrl();
        // Fixture: first block is a simple paragraph at the top of the document
        // so we exercise both the "chip above surface" case and the
        // "page-margin at top" case in one hit.
        const QMD = [
            '---',
            'format: q2-preview',
            '---',
            '',
            'First paragraph.',
            '',
            'Second paragraph.',
            '',
        ].join('\n');

        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'breadcrumb-geo.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'breadcrumb-geo.qmd');

        // Step 2: click the first block to open its editor.
        await iframe.locator('p[data-block-pool-id]').first().click();

        // Step 3: wait for both the textarea AND the chip.
        await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });
        const chip = iframe.locator('[data-testid="q2-breadcrumb-chip"]');
        await chip.waitFor({ timeout: 5000 });

        // The chip being visible is the proof that unlockDepthCursor took effect.
        const chipVisible = await chip.isVisible();
        expect(
            chipVisible,
            'BreadcrumbChip must be visible — unlockDepthCursor preference did not propagate',
        ).toBe(true);

        // Step 4: get bounding boxes from the real layout engine.
        const chipBox = await chip.boundingBox();
        const taBox = await iframe.locator('textarea').first().boundingBox();

        expect(chipBox, 'chip bounding box must be non-null').not.toBeNull();
        expect(taBox, 'textarea bounding box must be non-null').not.toBeNull();

        // TypeScript narrowing (already asserted above).
        if (!chipBox || !taBox) throw new Error('impossible — asserted above');

        console.log(
            `Chip box: y=${chipBox.y.toFixed(2)}, bottom=${(chipBox.y + chipBox.height).toFixed(2)}, height=${chipBox.height.toFixed(2)}\n` +
            `Textarea box: y=${taBox.y.toFixed(2)}`,
        );

        // Step 5: assertions.
        const TOL = 2;

        // (a) Chip bottom is AT OR ABOVE surface top — never occludes line 1.
        expect(
            chipBox.y + chipBox.height,
            `chip bottom (${(chipBox.y + chipBox.height).toFixed(2)}) must be ≤ textarea top (${taBox.y.toFixed(2)}) + ${TOL}px tolerance`,
        ).toBeLessThanOrEqual(taBox.y + TOL);

        // (b) Chip bottom is anchored close to the surface top (not floating far above).
        //     The gap must be less than ~one chip-gap (12 px) so the chip is still
        //     visually attached and the useLayoutEffect positioning is doing real work.
        expect(
            taBox.y - (chipBox.y + chipBox.height),
            `chip must be anchored near the surface top — gap (${(taBox.y - (chipBox.y + chipBox.height)).toFixed(2)}px) must be < 12px`,
        ).toBeLessThan(12);

        // (c) At the document top, the chip sits in the page margin (not clipped above
        //     the viewport — `top` is clamped to ≥ 0 in real CSS).
        expect(
            chipBox.y,
            `chip top (${chipBox.y.toFixed(2)}) must be ≥ 0 — not clipped above the viewport`,
        ).toBeGreaterThanOrEqual(0);

        // Step 6: close the editor.
        await iframe.locator('textarea').first().press('Escape');
    });
});
