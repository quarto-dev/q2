/**
 * E2E layout test for q2-preview: two child divs carrying inline
 * `style="flex: 1"` under a `style="display:flex"` parent must lay out
 * SIDE BY SIDE (Left content to the LEFT of Right content), the same as
 * `q2 render` HTML output — not stacked vertically.
 *
 * This is the real pipeline: parse → transform → UPDATE_AST → iframe render.
 * (An in-process RTL mount of a hand-built AST does NOT exercise this.)
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
    await iframe.locator('text=Left content').first().waitFor({ timeout: 15_000 });
    return iframe;
}

test.describe('q2-preview columns layout', () => {
    test.setTimeout(120000);

    test('two style=flex child divs lay out side by side (Left left of Right)', async ({ page }) => {
        const serverUrl = getServerUrl();
        const QMD = [
            '---',
            'format: q2-preview',
            '---',
            '',
            '## two columns',
            '',
            ':::: {style="display: flex; gap: 1rem;"}',
            '',
            '::: {style="flex: 1;"}',
            'Left content',
            ':::',
            '',
            '::: {style="flex: 1;"}',
            'Right content',
            ':::',
            '',
            '::::',
            '',
        ].join('\n');
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'columns.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'columns.qmd');

        const rects = await iframe.locator('body').evaluate(() => {
            const find = (t: string): DOMRect | null => {
                for (const p of Array.from(document.querySelectorAll('p'))) {
                    if (p.textContent?.includes(t)) return p.getBoundingClientRect();
                }
                return null;
            };
            const l = find('Left content');
            const r = find('Right content');
            const pack = (x: DOMRect | null) =>
                x && { left: x.left, right: x.right, top: x.top, bottom: x.bottom };
            return { l: pack(l), r: pack(r) };
        });

        expect(rects.l, 'Left content <p> found').not.toBeNull();
        expect(rects.r, 'Right content <p> found').not.toBeNull();
        const l = rects.l!;
        const r = rects.r!;

        // Side by side: Left ends (horizontally) before Right begins.
        expect(
            l.right,
            `Left.right=${l.right.toFixed(1)} must be <= Right.left=${r.left.toFixed(1)} (side by side, not stacked)`,
        ).toBeLessThanOrEqual(r.left + 1);

        // Same row: their vertical extents overlap (Left is NOT above Right).
        const verticalOverlap = Math.min(l.bottom, r.bottom) - Math.max(l.top, r.top);
        expect(
            verticalOverlap,
            `Left and Right must share a row (vertical overlap ${verticalOverlap.toFixed(1)}px > 0); negative means stacked`,
        ).toBeGreaterThan(0);
    });
});
