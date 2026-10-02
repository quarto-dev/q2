/**
 * P3.5 tier (ii) — SPA nesting-cursor e2e against the REAL `q2 preview` binary.
 *
 * Drives the compiled binary (target/debug/q2) via `startPreviewServer`, which
 * embeds the SPA + WASM through `include_dir!`. Asserts the §3a/§3b nesting-cursor
 * RESOLUTION that no existing q2-preview-spa spec covers:
 *
 *   - DEFAULT boot (no param, + `--allow-edit`, bd-9x3zbuj8): the nesting cursor
 *     is now ON by default (matching hub-client). A leaf-click on a blockquote
 *     child opens THAT child with a CLEAN, AST-regenerated buffer (no `>`
 *     markers), and the breadcrumb navigator chip is visible — proving leaf
 *     resolution + nested-buffer regeneration end to end through the binary's
 *     embedded SPA + WASM, with no query param required.
 *   - WITH the explicit `?nestingCursor=0` opt-out: clicking the blockquote
 *     opens the WHOLE quote WITH `>` markers (Phase-2 locked, prefixing-atomic)
 *     — proving the unlock can still be turned off.
 *
 * The boot path is covered at jsdom in
 * `q2-preview-spa/src/p3-2-nesting-cursor-spa.integration.test.tsx`; the
 * resolution assertions here are genuinely new and only meaningful against the
 * real binary + real WASM.
 *
 * Build chain prerequisite (the binary does NOT auto-rebuild the embedded
 * SPA/WASM):
 *   cd hub-client && npm run build:wasm
 *   cargo xtask build-q2-preview-spa
 *   cargo build -p quarto --bin q2
 *
 * Run via (from q2-preview-spa/):
 *   npx playwright test e2e/nesting-cursor.spec.ts --project=chromium
 */

import { test, expect, type Page } from '@playwright/test';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

// A multi-line blockquote child (qualifies for AST regeneration), bookended by
// plain paragraphs so the quote is a distinct prefixing container.
const FIXTURE_QMD = [
    '---',
    'format: q2-preview',
    '---',
    '',
    'Intro paragraph.',
    '',
    '> Quote line one.',
    '> Quote line two.',
    '',
    'Outro paragraph.',
    '',
].join('\n');

let server: PreviewServerHandle;

test.describe('P3.5 — SPA nesting-cursor resolution (real q2 preview binary)', () => {
    test.setTimeout(120_000);

    test.beforeEach(async () => {
        // --allow-edit so the SPA's edit surface is enabled (it fetches
        // /api/preview/config and gates editing on allowEdit).
        server = await startPreviewServer({
            fixtureFiles: [{ path: 'index.qmd', content: FIXTURE_QMD }],
            allowEdit: true,
        });
    });

    test.afterEach(async () => {
        await server?.stop();
    });

    /** Wait for the preview iframe to render the fixture's blockquote. */
    async function waitForBlockquote(page: Page): Promise<void> {
        await page.waitForFunction(
            () => {
                const inner = document.querySelector('iframe')?.contentDocument;
                return inner?.querySelector('blockquote[data-block-pool-id]') != null;
            },
            null,
            { timeout: 30_000 },
        );
    }

    test('default boot (no param): leaf-click opens the blockquote child in the RICH editor with the navigator INLINE in the toolbar (no overlap)', async ({ page }) => {
        // bd-9x3zbuj8: the nesting cursor is now ON by default — no query param.
        // Combined with rich-text being the SPA default, leaf-clicking a paragraph
        // opens the rich editor (ProseMirror), and the hierarchy navigator renders
        // INLINE in the toolbar row (Task 2) instead of as a floating chip.
        await page.goto(server.url);
        await waitForBlockquote(page);

        const iframe = page.frameLocator('iframe');

        // Leaf-click the blockquote CHILD (the inner paragraph). In unlocked mode
        // this resolves to the child Para, which opens in the rich editor.
        await iframe.locator('blockquote p[data-block-pool-id]').first().click();

        // The rich editor (ProseMirror) — NOT a textarea — must open, showing the
        // rendered quote text with no `>` source markers.
        const pm = iframe.locator('.q2-richtext-editor .ProseMirror').first();
        await pm.waitFor({ timeout: 10_000 });
        await expect
            .poll(async () => pm.textContent(), {
                timeout: 8000,
                message: 'rich editor should show the blockquote-child text',
            })
            .toContain('Quote line one.');
        const text = (await pm.textContent()) ?? '';
        expect(text, 'rich editor shows both lines').toContain('Quote line two.');
        expect(text, 'rich editor renders text without `>` markers').not.toContain('>');
        expect(text, 'leaf edit must not include the Intro paragraph').not.toContain('Intro paragraph.');

        // Task 2: the navigator renders INLINE inside the toolbar row (to the right
        // of the formatting buttons), and the standalone floating chip is suppressed
        // — so the two never overlap.
        const toolbar = iframe.locator('.q2-rt-toolbar').first();
        await expect(toolbar, 'rich-text toolbar must be present').toBeVisible({ timeout: 5000 });
        await expect(
            toolbar.locator('.q2-breadcrumb-out'),
            'navigator ◀ must be inside the toolbar row (inline breadcrumb)',
        ).toBeVisible();
        await expect(
            toolbar.locator('.q2-crumb').last(),
            'current crumb (¶) must be inside the toolbar row',
        ).toHaveText('¶');
        await expect(
            iframe.locator('[data-testid="q2-breadcrumb-chip"]'),
            'standalone floating chip must be suppressed when the inline breadcrumb shows',
        ).toHaveCount(0);

        await pm.press('Escape');
    });

    test('default nesting cursor with ?richText=0: leaf-click opens the blockquote child as a clean textarea buffer (no `>`)', async ({ page }) => {
        // With rich text opted OUT, the nested leaf opens in the textarea — this is
        // where the clean-buffer REGENERATION (markers stripped) is observable. Also
        // proves the nesting-cursor default-on is independent of the rich-text flag.
        const sep = server.url.includes('?') ? '&' : '?';
        await page.goto(`${server.url}${sep}richText=0`);
        await waitForBlockquote(page);

        const iframe = page.frameLocator('iframe');

        await iframe.locator('blockquote p[data-block-pool-id]').first().click();
        const ta = iframe.locator('textarea').first();
        await ta.waitFor({ timeout: 10_000 });

        await expect
            .poll(async () => ta.inputValue(), {
                timeout: 8000,
                message: 'textarea should contain the clean blockquote-child buffer',
            })
            .toContain('Quote line one.');

        const value = await ta.inputValue();
        expect(value, 'clean buffer must contain both lines').toContain('Quote line two.');
        expect(
            value,
            'clean nested buffer must NOT contain `>` markers (leaf resolution + regeneration)',
        ).not.toContain('>');
        expect(value, 'leaf edit must not include the Intro paragraph').not.toContain('Intro paragraph.');

        // The pop-up toolbar hosts the breadcrumb in plain/textarea mode too
        // (the standalone chip is gone); the nesting indicator must show by default.
        const toolbar = iframe.locator('.q2-rt-toolbar').first();
        await expect(toolbar, 'edit toolbar must be visible in plain mode').toBeVisible({ timeout: 5000 });
        await expect(
            toolbar.locator('.q2-crumb').last(),
            'type/nesting indicator must be visible by default',
        ).toBeVisible({ timeout: 5000 });
        await expect(
            iframe.locator('[data-testid="q2-breadcrumb-chip"]'),
            'standalone chip no longer exists',
        ).toHaveCount(0);

        await ta.press('Escape');
    });

    test('with ?nestingCursor=0 (opt-out): clicking the blockquote opens the whole quote with `>` (locked)', async ({ page }) => {
        // server.url already carries the CLI's `?page=index.qmd` query (previewServer
        // waitForUrl captures it), so append nestingCursor with the correct separator —
        // `${server.url}?nestingCursor=0` would produce the malformed `?page=index.qmd?nestingCursor=0`
        // (nestingCursor parses to null → defaults back ON, defeating the opt-out).
        const sep = server.url.includes('?') ? '&' : '?';
        await page.goto(`${server.url}${sep}nestingCursor=0`);
        await waitForBlockquote(page);

        const iframe = page.frameLocator('iframe');

        // Click inside the blockquote. In locked mode (explicit opt-out),
        // prefixing-atomic resolution opens the WHOLE quote as one buffer,
        // markers included.
        await iframe.locator('blockquote p[data-block-pool-id]').first().click();
        const ta = iframe.locator('textarea').first();
        await ta.waitFor({ timeout: 10_000 });

        await expect
            .poll(async () => ta.inputValue(), {
                timeout: 8000,
                message: 'textarea should contain the whole blockquote source',
            })
            .toContain('Quote line one.');

        const value = await ta.inputValue();
        expect(value, 'whole-quote buffer must contain both lines').toContain('Quote line two.');
        expect(
            value,
            'locked whole-quote buffer is a raw source slice — it MUST contain `>` markers',
        ).toContain('>');

        await ta.press('Escape');
    });
});
