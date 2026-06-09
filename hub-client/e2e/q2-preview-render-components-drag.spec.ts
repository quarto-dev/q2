/**
 * E2E tests for the drag render-component in q2-preview.
 *
 * drag.tsx lets any Div with x/y kv-attributes be dragged; on mouseup it
 * calls commitSubtreeEdit to persist the new coordinates back through
 * apply_node_edit → VFS → Automerge → re-render.
 *
 * Drag mechanism: mousedown on the grab handle, mousemove on the iframe
 * window, mouseup.  Playwright's page.mouse API fires real browser-level
 * pointer events that reach the iframe's window listeners.
 *
 * Tests:
 *   1. "drag renders div with grab handle" — smoke test, no edit.
 *   2. "dragging persists position through the round-trip" — fire a drag,
 *      assert transform style changed and QMD x/y attributes updated.
 */

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { test, expect, type Page, type FrameLocator } from '@playwright/test';
import {
    bootstrapProjectSet,
    createProjectOnServer,
    seedProjectInBrowser,
    getServerUrl,
} from './helpers/projectFactory';
import { waitForPreviewRender } from './helpers/previewExtraction';
import { assertAutomerge } from './helpers/assertAutomerge';

const FIXTURE_DIR = resolve(
    import.meta.dirname,
    '../../crates/quarto/tests/playwright-fixtures/q2-preview/render-components-drag',
);

const qmdContent = readFileSync(resolve(FIXTURE_DIR, 'render-components-drag.qmd'), 'utf-8');
const tsxContent = readFileSync(resolve(FIXTURE_DIR, 'drag.tsx'), 'utf-8');
const quartoYmlContent = readFileSync(resolve(FIXTURE_DIR, '_quarto.yml'), 'utf-8');

const TEST_ACTOR_ID = 'e2e7e1f02a30000000000000000007e3';

// ── Helpers ──────────────────────────────────────────────────────────

async function openDragFixture(page: Page): Promise<FrameLocator> {
    const serverUrl = getServerUrl();
    const indexDocId = await createProjectOnServer(serverUrl, [
        { path: '_quarto.yml', content: quartoYmlContent, contentType: 'text' },
        { path: 'drag.tsx', content: tsxContent, contentType: 'text' },
        { path: 'render-components-drag.qmd', content: qmdContent, contentType: 'text' },
    ]);
    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);
    await page.goto(`/#/p/${localId}/file/${encodeURIComponent('render-components-drag.qmd')}`);
    await waitForPreviewRender(page, { kind: 'q2-preview', timeout: 30_000 });
    const iframe = page.frameLocator('iframe[src*="q2-preview.html"]');
    // Wait for the drag component to mount (grab handle is the marker).
    await expect(iframe.locator('[style*="cursor: grab"]')).toBeVisible({ timeout: 30_000 });
    return iframe;
}

// ── Tests ─────────────────────────────────────────────────────────────

test.describe('q2-preview render-components-drag', () => {
    test.setTimeout(120_000);

    test.beforeEach(async ({ page }, testInfo) => {
        await page.addInitScript((id) => {
            (window as any).__QUARTO_TEST_ACTOR_ID__ = id;
        }, TEST_ACTOR_ID);
        if (testInfo.workerIndex > 0) await page.waitForTimeout(1000);
    });

    test('drag renders div with grab handle', async ({ page }) => {
        const iframe = await openDragFixture(page);

        // Grab handle is visible.
        await expect(iframe.locator('[style*="cursor: grab"]')).toBeVisible();

        // Content renders.
        await expect(iframe.locator('text=Drag me.')).toBeVisible();

        // No placeholder leakage.
        await expect(iframe.locator('div.q2-preview-placeholder')).toHaveCount(0);
    });

    test('dragging persists position through the round-trip', async ({ page }) => {
        const iframe = await openDragFixture(page);

        const handle = iframe.locator('[style*="cursor: grab"]');
        await expect(handle).toBeVisible();

        // Get the drag handle's screen position in the page coordinate space.
        const box = await handle.boundingBox();
        expect(box).not.toBeNull();
        const cx = box!.x + box!.width / 2;
        const cy = box!.y + box!.height / 2;

        // Perform the drag: mousedown on handle, move +80px right +60px down,
        // mouseup.  Using page.mouse so the events propagate to the iframe's
        // window listeners (mousemove/mouseup are on the iframe window, not
        // the handle element).
        await page.mouse.move(cx, cy);
        await page.mouse.down();
        await page.mouse.move(cx + 80, cy + 60, { steps: 10 });
        await page.mouse.up();

        // DOM: the draggable container's transform must have changed from
        // translate(0px, 0px) to a non-zero translation.
        const dragContainer = iframe.locator('[style*="cursor: grab"]').locator('..');
        await expect(async () => {
            const transform = await dragContainer.evaluate(
                (el) => (el as HTMLElement).style.transform,
            );
            expect(transform).not.toBe('translate(0px, 0px)');
            expect(transform).toMatch(/translate\(/);
        }).toPass({ timeout: 15_000 });

        // Automerge layer: the QMD must no longer have x=0 y=0 — the
        // coordinates were written back via commitSubtreeEdit.
        await assertAutomerge(page, 'render-components-drag.qmd', {
            lacks: ['x=0 y=0'],
        });
    });
});
