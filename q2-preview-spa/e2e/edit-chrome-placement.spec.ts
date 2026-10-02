/**
 * bd-pvcnea83 — floating edit chrome must not be cropped at the top of the
 * viewport. Editing the first block of a title-less document (flush against the
 * scroll-area top) previously clipped the chrome, which floats ABOVE the edit
 * box (`bottom:100%`), above the viewport top with no way to scroll up to it.
 * The fix flips the chrome BELOW the block when there's no room above.
 *
 * Real-binary e2e (drives target/debug/q2 via startPreviewServer). The pure
 * placement threshold is unit-tested in
 * ts-packages/preview-renderer/src/q2-preview/editChromeGeometry.test.ts; jsdom
 * rects are degenerate, so the actual flip is only observable here.
 *
 * Build chain prerequisite (the binary does NOT auto-rebuild the embedded SPA):
 *   cargo xtask build-q2-preview-spa
 *   cargo build -p quarto --bin q2
 */

import { test, expect, type Page } from '@playwright/test';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

let server: PreviewServerHandle;

/** Viewport-relative top of an element inside the preview iframe (0 = top of the
 *  visible scroll area). A clipped-above-the-top element has a negative top. */
async function iframeRectTop(page: Page, selector: string): Promise<number> {
    return page.frameLocator('iframe').locator(selector).first().evaluate(
        (el) => el.getBoundingClientRect().top,
    );
}

test.describe('bd-pvcnea83 — edit chrome flips below at the top of the viewport', () => {
    test.setTimeout(120_000);

    test.afterEach(async () => {
        await server?.stop();
    });

    test('rich-text toolbar: editing the first (title-less) paragraph flips the toolbar below, uncropped', async ({ page }) => {
        server = await startPreviewServer({
            allowEdit: true,
            fixtureFiles: [{
                path: 'index.qmd',
                // No title front matter: the first paragraph is flush against the top.
                content: 'First paragraph at the very top.\n\nSecond paragraph.\n',
            }],
        });
        await page.goto(server.url);
        const iframe = page.frameLocator('iframe');
        await page.waitForFunction(() => {
            const inner = document.querySelector('iframe')?.contentDocument;
            return inner?.querySelector('p[data-block-pool-id]') != null;
        }, null, { timeout: 30_000 });

        await iframe.locator('p[data-block-pool-id]').first().click();

        const toolbar = iframe.locator('.q2-rt-toolbar').first();
        await toolbar.waitFor({ timeout: 10_000 });

        // Flipped below (the collision-avoidance class) ...
        await expect(toolbar, 'toolbar must flip below at the top of the document')
            .toHaveClass(/q2-rt-toolbar-below/);
        // ... and consequently not cropped above the viewport top.
        await expect
            .poll(() => iframeRectTop(page, '.q2-rt-toolbar'), {
                timeout: 8000,
                message: 'toolbar top must be >= 0 (not clipped above the viewport)',
            })
            .toBeGreaterThanOrEqual(0);
    });

    test('plain-block toolbar: editing a first (title-less) code block leaves the toolbar uncropped', async ({ page }) => {
        server = await startPreviewServer({
            allowEdit: true,
            fixtureFiles: [{
                path: 'index.qmd',
                // First block is a code block (non-rich) → textarea + the same
                // pop-up toolbar (the standalone breadcrumb chip was folded into it).
                content: '```python\nx = 1\ny = 2\n```\n\nA paragraph after.\n',
            }],
        });
        await page.goto(server.url);
        const iframe = page.frameLocator('iframe');
        await page.waitForFunction(() => {
            const inner = document.querySelector('iframe')?.contentDocument;
            return inner?.querySelector('[data-block-pool-id]') != null;
        }, null, { timeout: 30_000 });

        await iframe.locator('[data-block-pool-id]').first().click();
        await iframe.locator('#q2-active-edit-region textarea').first().waitFor({ timeout: 10_000 });

        const toolbar = iframe.locator('.q2-rt-toolbar').first();
        await toolbar.waitFor({ timeout: 5000 });
        // No flip-class assertion: the edit box starts ~34px down, so the toolbar
        // (~25px) fits above it. Whichever side it lands on, it must not be clipped.
        await expect
            .poll(() => iframeRectTop(page, '.q2-rt-toolbar'), {
                timeout: 8000,
                message: 'plain-block toolbar top must be >= 0 (not clipped above the viewport)',
            })
            .toBeGreaterThanOrEqual(0);
    });
});
