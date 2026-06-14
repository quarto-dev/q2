/**
 * P3.5 tier (ii) — SPA depth-cursor e2e against the REAL `q2 preview` binary.
 *
 * Drives the compiled binary (target/debug/q2) via `startPreviewServer`, which
 * embeds the SPA + WASM through `include_dir!`. Asserts the §3a/§3b depth-cursor
 * RESOLUTION that no existing q2-preview-spa spec covers:
 *
 *   - WITH `?depthCursor=1` (+ `--allow-edit`): a leaf-click on a blockquote
 *     child opens THAT child with a CLEAN, AST-regenerated buffer (no `>`
 *     markers) — proving leaf resolution + nested-buffer regeneration end to
 *     end through the binary's embedded SPA + WASM.
 *   - WITHOUT the param: clicking the blockquote opens the WHOLE quote WITH `>`
 *     markers (Phase-2 locked, prefixing-atomic) — proving the unlock is gated.
 *
 * The boot path is covered at jsdom in
 * `q2-preview-spa/src/p3-2-depth-cursor-spa.integration.test.tsx`; the
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
 *   npx playwright test e2e/depth-cursor.spec.ts --project=chromium
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

test.describe('P3.5 — SPA depth-cursor resolution (real q2 preview binary)', () => {
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

    test('with ?depthCursor=1: leaf-click opens the blockquote child with a clean buffer (no `>`)', async ({ page }) => {
        // server.url already carries the CLI's `?page=index.qmd` query (previewServer
        // waitForUrl captures it), so append depthCursor with the correct separator —
        // `${server.url}?depthCursor=1` would produce the malformed `?page=index.qmd?depthCursor=1`
        // (depthCursor parses to null → the unlock silently never engages).
        const sep = server.url.includes('?') ? '&' : '?';
        await page.goto(`${server.url}${sep}depthCursor=1`);
        await waitForBlockquote(page);

        const iframe = page.frameLocator('iframe');

        // Leaf-click the blockquote CHILD (the inner paragraph). In unlocked
        // mode this resolves to the child, not the whole quote.
        await iframe.locator('blockquote p[data-block-pool-id]').first().click();
        const ta = iframe.locator('textarea').first();
        await ta.waitFor({ timeout: 10_000 });

        // The textarea must carry the CLEAN regenerated buffer: both quote lines,
        // and crucially NO `>` markers (regeneration stripped them).
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
        // And it must not have pulled in the surrounding paragraphs.
        expect(value, 'leaf edit must not include the Intro paragraph').not.toContain('Intro paragraph.');

        await ta.press('Escape');
    });

    test('without the param: clicking the blockquote opens the whole quote with `>` (locked)', async ({ page }) => {
        await page.goto(server.url);
        await waitForBlockquote(page);

        const iframe = page.frameLocator('iframe');

        // Click inside the blockquote. In locked mode (default), prefixing-atomic
        // resolution opens the WHOLE quote as one buffer, markers included.
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
