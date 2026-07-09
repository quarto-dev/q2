/**
 * bd-iwv3708i — a Pandoc `Quoted` inline must render in the rich-text block
 * editor as EDITABLE plaintext straight quotes (`"…"` / `'…'`), NOT as an opaque
 * `q2-chip` pill. Before this change, clicking a paragraph containing a quote
 * showed the quoted span as a non-editable monospace chip; now the quote
 * characters are literal, editable text and inner marks stay WYSIWYG.
 *
 * Real-binary e2e (drives target/debug/q2 via startPreviewServer). The pure
 * AST→ProseMirror mapping is unit-tested in
 * ts-packages/preview-renderer/src/q2-preview/richtext/quotedSeed.test.ts; the
 * qmd round-trip is guarded by richtext/roundtrip.test.ts. This spec confirms
 * the behavior through the actual embedded SPA the user runs.
 *
 * Build chain prerequisite (the binary does NOT auto-rebuild the embedded SPA):
 *   cargo xtask build-q2-preview-spa
 *   cargo build -p quarto --bin q2
 */

import { test, expect } from '@playwright/test';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

let server: PreviewServerHandle;

test.describe('bd-iwv3708i — Quoted renders as editable plaintext, not a chip', () => {
    test.setTimeout(120_000);

    test.afterEach(async () => {
        await server?.stop();
    });

    test('double- and single-quoted spans seed as editable text with no chip', async ({ page }) => {
        server = await startPreviewServer({
            allowEdit: true,
            fixtureFiles: [{
                path: 'index.qmd',
                // Both quote kinds in one paragraph so a single edit session covers both.
                content: 'He said "smart quotes" and a \'single one\' too.\n',
            }],
        });
        await page.goto(server.url);
        const iframe = page.frameLocator('iframe');
        await page.waitForFunction(() => {
            const inner = document.querySelector('iframe')?.contentDocument;
            return inner?.querySelector('p[data-block-pool-id]') != null;
        }, null, { timeout: 30_000 });

        // Click the paragraph to enter rich-text mode.
        await iframe.locator('p[data-block-pool-id]').first().click();
        const editor = iframe.locator('.q2-richtext-editor');
        await editor.waitFor({ timeout: 10_000 });

        // Neither the double- nor single-quoted span is chipped ...
        await expect(editor.locator('.q2-chip')).toHaveCount(0);
        // ... and both sets of straight quote characters are editable text.
        await expect(editor).toContainText('"smart quotes"');
        await expect(editor).toContainText("'single one'");
    });

    test('marks inside a quote stay WYSIWYG (no chip, real <strong>)', async ({ page }) => {
        server = await startPreviewServer({
            allowEdit: true,
            fixtureFiles: [{
                path: 'index.qmd',
                content: 'A quote with bold inside: "very **important** text".\n',
            }],
        });
        await page.goto(server.url);
        const iframe = page.frameLocator('iframe');
        await page.waitForFunction(() => {
            const inner = document.querySelector('iframe')?.contentDocument;
            return inner?.querySelector('p[data-block-pool-id]') != null;
        }, null, { timeout: 30_000 });

        await iframe.locator('p[data-block-pool-id]').first().click();
        const editor = iframe.locator('.q2-richtext-editor');
        await editor.waitFor({ timeout: 10_000 });

        // No chip anywhere; the quote chars are literal text ...
        await expect(editor.locator('.q2-chip')).toHaveCount(0);
        await expect(editor).toContainText('"very');
        await expect(editor).toContainText('text"');
        // ... and the inner Strong renders as a real WYSIWYG <strong> mark.
        await expect(editor.locator('strong')).toHaveText('important');
    });
});
