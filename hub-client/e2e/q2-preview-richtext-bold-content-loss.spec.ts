/**
 * bd-hafs0qho — reproduction: a rich-text BOLD commit on a tight-list item can
 * DROP the item's content.
 *
 * Observed manually in `q2 preview --allow-edit`: clicking a tight bullet-list
 * item opens the tiptap rich editor; select-all + bold + commit wrote an EMPTY
 * item to disk (`- banana` → `-`). Instrumentation showed the committed
 * ProseMirror doc had become `paragraph[hardBreak]` (text gone). The serializer
 * and the Rust commit path are both verified correct in isolation, and the pure
 * tiptap `selectAll()+toggleBold()+serialize` is ALSO clean — so the fault is in
 * the real browser event/commit flow, which only a real-keyboard e2e can drive
 * faithfully (synthetic DOM injection in jsdom/MCP-browser was confounded).
 *
 * This spec drives REAL keyboard events (Control+A, Control+B, Control+Enter)
 * through the rich editor and asserts the item KEEPS its content. It FAILS while
 * the bug is present (content lost) and passes once fixed.
 *
 * Harness mirrors q2-preview-inline-edit.spec.ts, with two changes:
 *   1. `richText: true` in the seeded preferences (default-on in prod, but the
 *      e2e preference object otherwise omits it → reads as off), so the rich
 *      editor opens instead of the textarea.
 *   2. Activation waits for the ProseMirror surface (`.ProseMirror`), not a
 *      textarea, and asserts it is the rich editor.
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

async function openFileWithRichText(
    page: Page,
    serverUrl: string,
    docId: string,
    filename: string,
): Promise<FrameLocator> {
    // richText ON + nesting cursor ON (the product default): clicking a tight
    // list item resolves the INNER Plain (rich editor), not the whole <ul>
    // (which, being a BulletList, would fall back to the textarea).
    await page.addInitScript(() => {
        localStorage.setItem(
            'quarto-hub:preferences',
            JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
                unlockNestingCursor: true,
                richText: true,
            }),
        );
    });
    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, docId, serverUrl);
    await page.goto(`/#/p/${localId}/file/${filename}`);
    await waitForPreviewRender(page, { kind: 'q2-preview', timeout: 30000 });
    const iframe = page.frameLocator('iframe[src*="q2-preview.html"]');
    await iframe.locator('[data-block-pool-id]').first().waitFor({ timeout: 15_000 });
    return iframe;
}

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

test.describe('bd-hafs0qho — rich-text bold commit content loss', () => {
    test.setTimeout(120000);

    test.beforeEach(async ({ page }, testInfo) => {
        if (testInfo.workerIndex > 0) await page.waitForTimeout(1000);
    });

    test('bolding a tight bullet-list item preserves its content', async ({ page }) => {
        const serverUrl = getServerUrl();
        const QMD =
            '---\nformat: q2-preview\n---\n\n## List\n\n- apple\n- banana\n- cherry\n';
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'lists.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFileWithRichText(page, serverUrl, docId, 'lists.qmd');
        await expect(iframe.locator('text=banana')).toBeVisible();

        // The tight-list <li> borrows the leading Plain's pool-id; clicking it
        // targets the Plain. Open the rich editor (ProseMirror), not a textarea.
        const li = iframe.locator('li[data-block-pool-id]', { hasText: 'banana' }).first();
        await li.click();
        const pm = iframe.locator('.ProseMirror');
        await pm.waitFor({ timeout: 5000 });
        // Guard: this must be the rich editor, not the textarea fallback.
        await expect(iframe.locator('textarea')).toHaveCount(0);
        await expect(pm).toContainText('banana');

        // Real keyboard: select all, bold, commit.
        await pm.press('ControlOrMeta+a');
        await pm.press('ControlOrMeta+b');
        await pm.press('ControlOrMeta+Enter');

        // The item must still contain its text, now bolded. The reported bug
        // dropped it, yielding an empty bullet.
        await assertAutomerge(page, 'lists.qmd', {
            contains: ['**banana**', 'apple', 'cherry'],
        });
    });

    test('bolding a paragraph preserves its content (shared-path control)', async ({ page }) => {
        // Control case on a Para (rich-editable before bd-7pxub583). If this
        // ALSO loses content, it confirms the bug is in the shared, type-agnostic
        // RichTextEditor path — not specific to Plain / list items.
        const serverUrl = getServerUrl();
        const QMD =
            '---\nformat: q2-preview\n---\n\n## Para\n\nHello world paragraph.\n';
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'para.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFileWithRichText(page, serverUrl, docId, 'para.qmd');
        await expect(iframe.locator('text=Hello world paragraph.')).toBeVisible();

        await iframe.locator('p[data-block-pool-id]', { hasText: 'Hello world' }).first().click();
        const pm = iframe.locator('.ProseMirror');
        await pm.waitFor({ timeout: 5000 });
        await expect(iframe.locator('textarea')).toHaveCount(0);

        await pm.press('ControlOrMeta+a');
        await pm.press('ControlOrMeta+b');
        await pm.press('ControlOrMeta+Enter');

        await assertAutomerge(page, 'para.qmd', {
            contains: ['**Hello world paragraph.**'],
        });
    });
});
