/**
 * T15 — G12 scrolljack rule: `overflow-y` is `'auto'` only when the textarea
 * content is genuinely clipped (real browser layout).
 *
 * Rule: a collapsed textarea whose source SPILLS past its collapsed height sets
 * `overflow-y: auto` (internal scroll is wanted — the user can scroll the code).
 * An EXPANDED textarea (sized to fit) or a FITTING block (source height ≤ collapsed
 * box height) sets `overflow-y: hidden` so the wheel passes through to the page
 * (no scrolljack).
 *
 * jsdom reports `scrollHeight === clientHeight === 0` for all textareas, so this
 * check is vacuous there and must be asserted at the real-browser tier.
 *
 * Three cases:
 *  (a) Collapsed, spilling code block  →  overflowY === 'auto'
 *  (b) Same block, expanded (second click)  →  overflowY === 'hidden'
 *  (c) 1-liner / fitting block  →  overflowY === 'hidden'
 *
 * FAIL-ON-REVERT (T15):
 *  Force `ta.style.overflowY = 'auto'` unconditionally (remove the clip test) →
 *  (b)/(c) read 'auto' → RED.
 *
 * Run via:
 *   cd hub-client && npx playwright test e2e/q2-preview-scrolljack.spec.ts --project=chromium --workers=1
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

/** A code block with many lines so its collapsed height is much less than the source height. */
const SPILLING_CODE_LINES = Array.from({ length: 20 }, (_, i) => `line_${i + 1} = ${i + 1};`).join('\n');

// QMD with:
//  - a multi-line code block (spills when collapsed)
//  - a 1-line paragraph (always fits)
const QMD = [
    '---',
    'format: q2-preview',
    '---',
    '',
    '```python',
    SPILLING_CODE_LINES,
    '```',
    '',
    'Short paragraph.',
    '',
].join('\n');

/** Read the current `overflow-y` style of the active textarea inside the iframe. */
async function readOverflowY(iframe: FrameLocator): Promise<string> {
    return iframe.locator('textarea').first().evaluate((el) => {
        return getComputedStyle(el).overflowY;
    });
}

test.describe('T15 (G12) — scrolljack rule: overflow-y only when genuinely clipped (real browser)', () => {
    test.setTimeout(120000);

    test.beforeEach(async ({ page }, testInfo) => {
        if (testInfo.workerIndex > 0) await page.waitForTimeout(1000);
        // Enable unlock mode so nesting cursor features are available (consistent with other specs).
        await page.addInitScript(() => {
            localStorage.setItem('quarto-hub:preferences', JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
                unlockNestingCursor: true,
            }));
        });
    });

    test('(a) collapsed spilling code block → overflowY auto; (b) expanded → hidden; (c) 1-liner → hidden', async ({ page }) => {
        const serverUrl = getServerUrl();
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'scrolljack.qmd', content: QMD, contentType: 'text' },
        ]);
        const iframe = await openFile(page, serverUrl, docId, 'scrolljack.qmd');

        // Wait for the document to render (code block should be visible).
        await iframe.locator('[data-block-pool-id]').first().waitFor({ timeout: 15_000 });

        // ── (a) Collapsed, spilling code block ──────────────────────────────
        // Click the code block to activate the editor. It opens COLLAPSED.
        // The source has 20 lines, so scrollHeight >> clientHeight → clips.
        const codeBlock = iframe.locator('[data-block-pool-id]').first();
        await codeBlock.click();
        const ta = iframe.locator('textarea').first();
        await ta.waitFor({ timeout: 10_000 });

        // Verify it opened collapsed (no data-expanded attribute).
        const isExpandedAtOpen = await ta.evaluate((el) => el.hasAttribute('data-expanded'));
        console.log(`code block opened collapsed: ${!isExpandedAtOpen}`);
        expect(isExpandedAtOpen, 'code block must open collapsed for the scrolljack test to be meaningful').toBe(false);

        // Check geometry: the code block must genuinely clip (source > collapsed box).
        const geometry = await ta.evaluate((el) => {
            const ta = el as HTMLTextAreaElement;
            return {
                scrollHeight: ta.scrollHeight,
                clientHeight: ta.clientHeight,
                overflowY: getComputedStyle(ta).overflowY,
            };
        });
        console.log(`(a) collapsed: scrollH=${geometry.scrollHeight} clientH=${geometry.clientHeight} overflowY=${geometry.overflowY}`);

        // The key assertion: a collapsed, spilling code block must use 'auto' so the
        // user can scroll its clipped window. The SCROLLJACK_TOLERANCE_PX=6 absorbs
        // small rounding; a 20-line block far exceeds this tolerance.
        expect(
            geometry.overflowY,
            `(a) collapsed spilling code block: overflowY must be 'auto' (clips are wanted)`,
        ).toBe('auto');

        // ── (b) Expand the editor (second click) → overflowY becomes 'hidden' ──
        // After expansion the textarea is sized to fit all content, so scrollHeight
        // ≤ clientHeight + tolerance → should switch to 'hidden'.
        await ta.click();
        await page.waitForTimeout(200); // let the layout effect run

        const isExpandedAfterClick = await ta.evaluate((el) => el.hasAttribute('data-expanded'));
        console.log(`expanded after second click: ${isExpandedAfterClick}`);
        expect(isExpandedAfterClick, 'second click must expand the editor').toBe(true);

        const overflowAfterExpand = await readOverflowY(iframe);
        console.log(`(b) expanded: overflowY=${overflowAfterExpand}`);
        expect(
            overflowAfterExpand,
            `(b) expanded editor: overflowY must be 'hidden' (content fits, no scrolljack)`,
        ).toBe('hidden');

        // Close this editor before opening the next.
        await ta.press('Escape');
        await page.waitForTimeout(200);

        // ── (c) 1-liner / fitting block → always 'hidden' ──────────────────
        // The paragraph "Short paragraph." is a single short line. Its source
        // fits within any reasonable collapsed height → no clip → 'hidden'.
        const shortPara = iframe.getByText('Short paragraph.', { exact: true });
        await shortPara.click();
        const taShort = iframe.locator('textarea').first();
        await taShort.waitFor({ timeout: 10_000 });

        const overflowShort = await readOverflowY(iframe);
        console.log(`(c) 1-liner: overflowY=${overflowShort}`);
        expect(
            overflowShort,
            `(c) 1-liner / fitting paragraph: overflowY must be 'hidden' (never clips, no scrolljack)`,
        ).toBe('hidden');

        await taShort.press('Escape');
    });
});
