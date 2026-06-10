/**
 * E2E tests for the kanban render-component in q2-preview.
 *
 * kanban.tsx uses Plan 2b's `commitSubtreeEdit` (via `usePreviewEdit`)
 * so drag-and-drop reorders round-trip through `apply_node_edit` → VFS →
 * Automerge → re-render.
 *
 * Test layout:
 *   1. "kanban renders columns and items" — smoke test that the component
 *      loads, columns appear, and no placeholder leakage.
 *   2. "moving a card between columns persists through the round-trip" —
 *      drag "item one" from "backlog" to "doing"; assert DOM update and
 *      QMD change at the Automerge layer.
 *
 * Drag mechanism: kanban.tsx uses HTML5 drag events (draggable,
 * onDragStart, onDragOver, onDrop).  Playwright's `locator.dragTo()`
 * fires the correct sequence; the column calls e.preventDefault() in
 * onDragOver so the drop event fires.
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
    '../../crates/quarto/tests/playwright-fixtures/q2-preview/render-components-kanban',
);

const qmdContent = readFileSync(resolve(FIXTURE_DIR, 'render-components-kanban.qmd'), 'utf-8');
const tsxContent = readFileSync(resolve(FIXTURE_DIR, 'kanban.tsx'), 'utf-8');
const quartoYmlContent = readFileSync(resolve(FIXTURE_DIR, '_quarto.yml'), 'utf-8');

const TEST_ACTOR_ID = 'e2e7e1f02a30000000000000000007e2';

// ── Helpers ──────────────────────────────────────────────────────────

async function openKanbanFixture(page: Page): Promise<FrameLocator> {
    const serverUrl = getServerUrl();
    const indexDocId = await createProjectOnServer(serverUrl, [
        { path: '_quarto.yml', content: quartoYmlContent, contentType: 'text' },
        { path: 'kanban.tsx', content: tsxContent, contentType: 'text' },
        { path: 'render-components-kanban.qmd', content: qmdContent, contentType: 'text' },
    ]);
    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);
    await page.goto(`/#/p/${localId}/file/${encodeURIComponent('render-components-kanban.qmd')}`);
    await waitForPreviewRender(page, { kind: 'q2-preview', timeout: 30_000 });
    const iframe = page.frameLocator('iframe[src*="q2-preview.html"]');
    // Wait for the kanban board to render (h3 columns).
    await expect(iframe.locator('h3').filter({ hasText: 'backlog' })).toBeVisible({ timeout: 30_000 });
    return iframe;
}

// ── Tests ─────────────────────────────────────────────────────────────

test.describe('q2-preview render-components-kanban', () => {
    test.setTimeout(120_000);

    test.beforeEach(async ({ page }, testInfo) => {
        await page.addInitScript((id) => {
            (window as any).__QUARTO_TEST_ACTOR_ID__ = id;
        }, TEST_ACTOR_ID);
        if (testInfo.workerIndex > 0) await page.waitForTimeout(1000);
    });

    test('kanban renders columns and items', async ({ page }) => {
        const iframe = await openKanbanFixture(page);

        // Both columns visible.
        await expect(iframe.locator('h3').filter({ hasText: 'backlog' })).toBeVisible();
        await expect(iframe.locator('h3').filter({ hasText: 'doing' })).toBeVisible();

        // Items visible.
        await expect(iframe.locator('[draggable="true"]').filter({ hasText: 'item one' })).toBeVisible();
        await expect(iframe.locator('[draggable="true"]').filter({ hasText: 'item two' })).toBeVisible();
        await expect(iframe.locator('[draggable="true"]').filter({ hasText: 'item three' })).toBeVisible();

        // No placeholder leakage — kanban must override Div.
        await expect(iframe.locator('div.q2-preview-placeholder')).toHaveCount(0);
    });

    test('moving a card between columns persists through the round-trip', async ({ page }) => {
        const iframe = await openKanbanFixture(page);

        // Locate the draggable "item one" card.
        const itemOne = iframe.locator('[draggable="true"]').filter({ hasText: 'item one' });
        await expect(itemOne).toBeVisible();

        // Locate the "doing" column drop target.  The column div is the most
        // specific (deepest) div that directly contains the "doing" h3, so
        // use .last() to avoid matching outer wrapper containers that also
        // happen to contain the h3 transitively.
        const doingColumn = iframe.locator('div').filter({
            has: iframe.locator('h3').filter({ hasText: 'doing' }),
        }).last();
        await expect(doingColumn).toBeVisible();

        // Drag "item one" to the "doing" column.
        await itemOne.dragTo(doingColumn);

        // DOM: "item one" must appear in the doing column's items wrapper —
        // specifically the same direct-parent div as "item three" (which
        // starts in "doing").  We find the items wrapper by looking for the
        // most-specific div that already contains "item three".
        const doingItemsWrapper = iframe.locator('div').filter({
            has: iframe.locator('[draggable="true"]').filter({ hasText: /^item three$/ }),
        }).last();
        await expect(async () => {
            await expect(
                doingItemsWrapper.locator('[draggable="true"]').filter({ hasText: /^item one$/ }),
            ).toBeVisible();
        }).toPass({ timeout: 15_000 });

        // Automerge layer: the QMD must reflect that "item one" left "backlog".
        // In the original fixture the two backlog items appear consecutively:
        //   * item one
        //   * item two
        // After the move that sequence must be gone — "item one" is now in
        // the "doing" section, so "item two" is the only backlog item.
        await assertAutomerge(page, 'render-components-kanban.qmd', {
            contains: ['item two', 'item three'],
            lacks: ['item one\n* item two'],
        });
    });
});
