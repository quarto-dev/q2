/**
 * E2E tests for q2-preview inline editing (Plan 2b).
 *
 * Round-trip verified: click block → textarea → Ctrl+Enter commit →
 * Automerge updated.
 *
 * Updated from Plan 1 (contenteditable) to Plan 2b (data-block-pool-id
 * + textarea + commitTextEdit channel). The DOM interaction is now:
 *   1. Click `<tag>[data-block-pool-id]` to activate the textarea.
 *   2. `fill()` the new text (clears + sets the textarea value).
 *   3. Press Ctrl+Enter to commit (keyboard shortcut in useEditableBlock).
 *
 * Implementation notes for future tests in this area:
 *  - Monaco CDN interception is handled automatically by bootstrapProjectSet;
 *    no per-test setup needed.
 *  - Verify via getFileContent() (Automerge layer) rather than DOM text,
 *    since the DOM may lag the Automerge update by a render tick.
 *  - Ctrl+Enter commit is more reliable than blur in Playwright automation,
 *    because clicking a blur target activates edit on it (Plan 2b's
 *    delegated pointer handler runs on any [data-block-pool-id] click).
 *  - Tests run in parallel (serial mode removed). Three changes make this safe:
 *    1. createProjectOnServer uses peerTimeoutMs=10000 so documents are created
 *       in online mode and flush to the hub immediately (no background-sync race).
 *    2. bootstrapProjectSet stubs /auth/me + sets MonacoEnvironment.getWorkerUrl
 *       + serves monaco-editor from local devDep to prevent AMD init races.
 *    3. beforeEach stagger: workerIndex>0 waits 1 s so both browsers don't enter
 *       Monaco's AMD init window at exactly the same instant.
 *  - Prerequisites: VITE_E2E=1 npm run build, no hub on port 3031 (test port).
 *
 * Preview-view test (monaco-absent path):
 *  - bootstrapProjectSet registers a CDN→local-files Monaco intercept.
 *    The preview-view test registers a CDN-abort route AFTER that; Playwright
 *    evaluates routes LIFO, so the abort fires first and Monaco never loads.
 *    This exercises the inline-edit path where the hub-client editor is absent
 *    but the preview-iframe textarea still works.
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

/** Set up the project, navigate, and wait for Monaco + preview (+ edit affordances) to be ready. */
async function openFile(
    page: Page,
    serverUrl: string,
    docId: string,
    filename: string,
): Promise<FrameLocator> {
    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, docId, serverUrl);
    await page.goto(`/#/p/${localId}/file/${filename}`);
    // Monaco must be ready for the write-back path.
    await expect(page.locator('.view-lines').first()).toBeVisible({ timeout: 30000 });
    await waitForPreviewRender(page, { kind: 'q2-preview', timeout: 30000 });
    const iframe = page.frameLocator('iframe[src*="q2-preview.html"]');
    // Wait for Plan 2b edit affordances (sourceIndex built → data-block-pool-id set).
    await iframe.locator('[data-block-pool-id]').first().waitFor({ timeout: 15_000 });
    return iframe;
}

/**
 * Activate the first matching block for editing, replace its text, then
 * commit with Ctrl+Enter.
 *
 * Plan 2b mechanism: click [data-block-pool-id] → textarea → fill + Ctrl+Enter.
 * This avoids clicking a blur target (which would activate edit on it via the
 * delegated pointer handler).
 */
async function editBlock(
    iframe: FrameLocator,
    tag: string,
    newText: string,
): Promise<void> {
    await iframe.locator(`${tag}[data-block-pool-id]`).first().click();
    const ta = iframe.locator('textarea').first();
    await ta.waitFor({ timeout: 5000 });
    await ta.fill(newText);
    await ta.press('Control+Enter');
}

/** Poll Automerge until the file satisfies all content checks. */
async function assertAutomerge(
    page: Page,
    filename: string,
    { contains = [], lacks = [] }: { contains?: string[]; lacks?: string[] },
): Promise<void> {
    await expect(async () => {
        const text = await page.evaluate(async f => {
            await window.__quartoTestReady;
            return window.__quartoTest!.wasmRenderer.getFileContent(f) as string | null;
        }, filename);
        expect(text).not.toBeNull();
        for (const s of contains) expect(text).toContain(s);
        for (const s of lacks) expect(text).not.toContain(s);
    }).toPass({ timeout: 10000 });
}

test.describe('q2-preview inline editing', () => {
    test.setTimeout(120000);

    // Stagger worker start times so two parallel browser contexts don't
    // race through Monaco's AMD initialisation at exactly the same instant.
    // workerIndex 0 starts immediately; workerIndex 1 waits 1 s.
    test.beforeEach(async ({ page }, testInfo) => {
        if (testInfo.workerIndex > 0) {
            await page.waitForTimeout(1000);
        }
    });

    test('editing a paragraph updates the Automerge document', async ({ page }) => {
        const serverUrl = getServerUrl();
        const QMD =
            '---\nformat: q2-preview\n---\n\n## Section\n\nFirst paragraph.\n\nSecond paragraph.\n';
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'doc.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'doc.qmd');
        await expect(iframe.locator('text=First paragraph.')).toBeVisible();

        await editBlock(iframe, 'p', 'Edited paragraph.');

        await assertAutomerge(page, 'doc.qmd', {
            contains: ['Edited paragraph.', 'Second paragraph.'],
            lacks: ['First paragraph.'],
        });
    });

    test('editing a heading updates the Automerge document', async ({ page }) => {
        const serverUrl = getServerUrl();
        const QMD = '---\nformat: q2-preview\n---\n\n## My Heading\n\nA paragraph.\n';
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'heading.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'heading.qmd');
        await expect(iframe.locator('text=My Heading')).toBeVisible();

        await editBlock(iframe, 'h2', 'New Heading');

        await assertAutomerge(page, 'heading.qmd', {
            contains: ['New Heading'],
            lacks: ['My Heading'],
        });
    });

    test('editing in Preview view before Monaco mounts updates the Automerge document', async ({ page }) => {
        const serverUrl = getServerUrl();
        const QMD =
            '---\nformat: q2-preview\n---\n\n## Section\n\nOriginal text.\n\nOther paragraph.\n';
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'preview-view.qmd', content: QMD, contentType: 'text' },
        ]);

        await bootstrapProjectSet(page, serverUrl);
        const localId = await seedProjectInBrowser(page, docId, serverUrl);

        // Abort all Monaco CDN requests so editorRef.current stays null.
        // bootstrapProjectSet already registered a CDN→local-files route;
        // this abort route is registered AFTER it, so Playwright's LIFO
        // evaluation runs the abort first.
        await page.route(
            '**/cdn.jsdelivr.net/npm/monaco-editor@*/min/vs/**',
            route => route.abort(),
        );

        await page.goto(`/#/p/${localId}/file/preview-view.qmd`);
        // Switch to Preview view without waiting for Monaco.
        await page.getByRole('button', { name: 'Preview view' }).click();

        await waitForPreviewRender(page, { kind: 'q2-preview', timeout: 30000 });
        const iframe = page.frameLocator('iframe[src*="q2-preview.html"]');
        await iframe.locator('[data-block-pool-id]').first().waitFor({ timeout: 15_000 });
        await expect(iframe.locator('text=Original text.')).toBeVisible();

        await editBlock(iframe, 'p', 'Edited text.');

        await assertAutomerge(page, 'preview-view.qmd', {
            contains: ['Edited text.', 'Other paragraph.'],
            lacks: ['Original text.'],
        });
    });
});
