/**
 * §3 mode-aware hover outline acceptance — locked mode, real browser.
 *
 * The reported bug: in LOCKED mode (nesting cursor off — the default) the hover
 * outline highlighted the deepest leaf `<li>`/`<p>`, but a locked-mode click
 * activates the OUTER BLOCK (the whole `<ul>`). The highlight promised a
 * granularity the click did not deliver. §3 makes the outline use the same mode
 * branch as activation: `resolveOuterBlock` in locked mode.
 *
 * Hover a leaf deep inside the list → the box-shadow outline must land on the
 * OUTERMOST `<ul>` (the outer block a click would activate via resolveOuterBlock),
 * NOT an inner nested `<ul>`. (List-item text resolves to a `<ul>` either way, so
 * the discriminator is outermost-vs-innermost, not ul-vs-leaf: the pre-fix raw
 * outline lands on the *innermost* enclosing `<ul>`.)
 *
 * Real browser: the outline is an inline box-shadow set on a real
 * `[data-block-pool-id]` element under a real `pointermove`; `resolveOuterBlock`'s
 * prefixing-container resolution needs real layout/visibility.
 *
 * Fail-on-revert: restore the leaf-only outline (outline the raw leaf instead of
 * resolveOuterBlock) → the `<p>` carries the box-shadow and the `<ul>` does not →
 * the assertions go RED.
 *
 * Run via:
 *   cd hub-client && VITE_E2E=1 npm run build
 *   npx playwright test e2e/q2-preview-locked-hover.spec.ts --project=chromium --workers=1
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

// A 3-level list so a deep leaf has its OWN pool-id distinct from the outer <ul>
// (a flat list's item text resolves straight to the <ul>, so it can't discriminate).
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

test.describe('§3 — locked-mode hover outlines the outer block (real browser)', () => {
    test.setTimeout(120000);

    test.beforeEach(async ({ page }, testInfo) => {
        if (testInfo.workerIndex > 0) await page.waitForTimeout(1000);
    });

    test('hovering a deep list leaf outlines a <ul> (outer block), never the leaf <p>', async ({ page }) => {
        // LOCKED mode: unlockNestingCursor is intentionally absent (the default).
        await page.addInitScript(() => {
            localStorage.setItem('quarto-hub:preferences', JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
            }));
        });

        const serverUrl = getServerUrl();
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'locked-hover.qmd', content: QMD, contentType: 'text' },
        ]);
        const iframe = await openFile(page, serverUrl, docId, 'locked-hover.qmd');

        await iframe.locator('ul[data-block-pool-id]').first().waitFor({ timeout: 10_000 });

        // Hover the DEEPEST leaf ("sub-sub-item") — its own pool-id <p> is distinct
        // from the outer <ul> that a locked-mode click would activate. ("sub-sub-item"
        // is an unambiguous substring; "sub-item" is contained in it.)
        await iframe.getByText('sub-sub-item', { exact: true }).hover();

        // Inspect the pool-id element(s) currently carrying an inline box-shadow.
        // The outline must land on EXACTLY the OUTERMOST <ul> — a <ul> with no
        // pool-id ancestor (what resolveOuterBlock returns). The pre-fix raw outline
        // lands on the innermost enclosing <ul>, which HAS a pool-id ancestor.
        const result = await iframe.locator('[data-block-pool-id]').evaluateAll((els) => {
            const shadowed = (els as HTMLElement[]).filter((e) => e.style.boxShadow !== '');
            return shadowed.map((e) => ({
                tag: e.tagName,
                hasPoolIdAncestor: e.parentElement?.closest('[data-block-pool-id]') != null,
            }));
        });
        console.log(`locked hover → outlined: ${JSON.stringify(result)}`);

        expect(result.length, 'exactly one element is outlined').toBe(1);
        expect(result[0].tag, 'the outlined element is a <ul> outer block').toBe('UL');
        expect(
            result[0].hasPoolIdAncestor,
            'the outline lands on the OUTERMOST <ul> (no pool-id ancestor) — the pre-fix bug outlines an inner <ul>',
        ).toBe(false);
    });
});
