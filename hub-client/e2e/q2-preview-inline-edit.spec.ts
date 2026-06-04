/**
 * E2E tests for q2-preview inline editing (target-incremental-writes Phase 6).
 *
 * Round-trip verified: click block → type → blur → Automerge updated.
 *
 * Implementation notes for future tests in this area:
 *  - Monaco CDN interception is handled automatically by bootstrapProjectSet;
 *    no per-test setup needed.
 *  - DOM assertions on edited contentEditable elements are unreliable: React
 *    doesn't always reconcile their innerHTML, leaving artefacts from user
 *    typing. Verify via getFileContent() (Automerge layer) instead.
 *  - Blur (click outside) is more reliable than Enter for committing edits in
 *    Playwright automation.
 *  - serial mode avoids hub peer-connection timeouts when both tests compete
 *    for the same sync server concurrently.
 *  - Prerequisites: VITE_E2E=1 npm run build, no hub on port 3031 (test port),
 *    dev hub uses 3030.
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

/** Set up the project, navigate, and wait for Monaco + preview to be ready. */
async function openFile(
    page: Page,
    serverUrl: string,
    docId: string,
    filename: string,
): Promise<FrameLocator> {
    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, docId, serverUrl);
    await page.goto(`/#/p/${localId}/file/${filename}`);
    // Monaco must be ready for the write-back path (handleContentRewrite needs editorRef).
    await expect(page.locator('.view-lines').first()).toBeVisible({ timeout: 30000 });
    await waitForPreviewRender(page, { kind: 'q2-preview', timeout: 30000 });
    return page.frameLocator('iframe[src*="q2-preview.html"]');
}

/** Click an editable block, replace its text, then blur to commit. */
async function editBlock(
    iframe: FrameLocator,
    tag: string,
    newText: string,
    blurTarget: string,
): Promise<void> {
    await iframe.locator(`${tag}[title="Click to edit"]`).first().click();
    const el = iframe.locator(`${tag}[contenteditable="true"]`).first();
    await el.waitFor({ timeout: 5000 });
    await el.click();
    await el.press('Meta+a');
    await el.pressSequentially(newText);
    await iframe.locator(blurTarget).click();
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

test.describe.configure({ mode: 'serial' });

test.describe('q2-preview inline editing', () => {
    test.setTimeout(120000);

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

        await editBlock(iframe, 'p', 'Edited paragraph.', 'h2');

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

        await editBlock(iframe, 'h2', 'New Heading', 'p');

        await assertAutomerge(page, 'heading.qmd', {
            contains: ['New Heading'],
            lacks: ['My Heading'],
        });
    });
});
