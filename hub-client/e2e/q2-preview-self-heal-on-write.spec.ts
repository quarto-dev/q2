/**
 * Browser-tier regression guard: self-heal KEEP across a real collaborator shift.
 *
 * ## What this file guards (browser-only increment)
 *
 * The self-heal KEEP *logic* (findReanchorCandidate choosing KEEP over DROP) is
 * already covered by `p2-3b-real.integration.test.tsx` §3 case (b) in jsdom.
 * That test owns the logic. This Playwright spec owns the ONE assertion jsdom
 * structurally cannot make:
 *
 *   "After a REAL Automerge re-render that shifts block byte offsets, the editor
 *    remains mounted (textarea still visible), the dirty draft is still present
 *    in the textarea, and the file on disk was never corrupted by a stale write."
 *
 * jsdom never fires native blur on teardown, so the "editor survives real blur"
 * path is unreachable there. Chromium DOES fire it — and PreviewRoot's
 * `useLayoutEffect` self-heal must re-anchor before the blur can commit the stale
 * range. That is the only claim under test here.
 *
 * Do NOT add logic tests here; they belong in p2-3b-real.
 *
 * ## Run (after build prerequisites)
 *
 *   # Prerequisites (run once when ts-packages/preview-renderer changed):
 *   #   cd ts-packages/preview-renderer && npm run build
 *   #   cd hub-client && VITE_E2E=1 npm run build
 *   cd hub-client
 *   npx playwright test q2-preview-self-heal-on-write.spec.ts --project=chromium
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

// ---------------------------------------------------------------------------
// Fixture QMD — several paragraphs so we have a MIDDLE block to edit
// and room to insert a new block ABOVE it.
//
// Byte layout (after the YAML front-matter + blank line):
//   Para 0 — "First paragraph."       ← will be shifted down by the insert
//   Para 1 — "Second paragraph."      ← will be shifted down by the insert
//   Para 2 — "Third paragraph."       ← MIDDLE block we open for editing
//   Para 3 — "Fourth paragraph."      ← unrelated
//   Para 4 — "Fifth paragraph."       ← unrelated
// ---------------------------------------------------------------------------
const FIXTURE_QMD = [
    '---',
    'format: q2-preview',
    '---',
    '',
    'First paragraph.',
    '',
    'Second paragraph.',
    '',
    'Third paragraph.',
    '',
    'Fourth paragraph.',
    '',
    'Fifth paragraph.',
    '',
].join('\n');

/** Dirty draft we type in the middle textarea — distinctive marker. */
const DRAFT = 'DRAFT_SURVIVES_SELFHEAL';

/**
 * New file content that a "collaborator" injects:
 * inserts a brand-new paragraph at the very TOP (before "First paragraph."),
 * shifting all existing block byte offsets downward.
 */
const COLLABORATOR_INSERT_QMD = [
    '---',
    'format: q2-preview',
    '---',
    '',
    'COLLABORATOR_INSERT_ABOVE.',
    '',
    'First paragraph.',
    '',
    'Second paragraph.',
    '',
    'Third paragraph.',
    '',
    'Fourth paragraph.',
    '',
    'Fifth paragraph.',
    '',
].join('\n');

// ---------------------------------------------------------------------------
// Helpers (same pattern as the exemplar specs)
// ---------------------------------------------------------------------------

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

async function getFileContent(page: Page, filename: string): Promise<string | null> {
    return page.evaluate(async (f) => {
        await window.__quartoTestReady;
        return window.__quartoTest!.wasmRenderer.getFileContent(f) as string | null;
    }, filename);
}

// ---------------------------------------------------------------------------
// Test suite
// ---------------------------------------------------------------------------

test.describe('self-heal KEEP (browser tier) — green regression guard (bd-k1evg0g1, fixed 2026-06-19)', () => {
    test.setTimeout(120000);

    test.beforeEach(async ({ page }, testInfo) => {
        if (testInfo.workerIndex > 0) {
            await page.waitForTimeout(1000);
        }
    });

    test('self-heal KEEP: editor survives a real collaborator shift (real browser, real blur) without corruption', async ({ page }) => {
        // GREEN REGRESSION GUARD (was a known-failing tripwire for bd-k1evg0g1,
        // deferred 2026-06-13; FIXED 2026-06-19).
        //
        // This test asserts the real browser-tier self-heal-KEEP guarantee. It used
        // to FAIL because findReanchorCandidate (outerBlocks.ts) chose its re-anchor
        // candidate by OFFSET ("nearest r[0] >= anchorR0") and then content-verified:
        // on a collaborator insert-above large enough that a PRECEDING block shifts to
        // >= the active block's OLD anchorR0, that nearest candidate was the wrong
        // (preceding) block, content-verify failed, and the editor DROPPED instead of
        // re-anchoring (KEEP). The fix made findReanchorCandidate CONTENT-first (match
        // anchorSlice, then pick the nearest offset in either direction), which both
        // scans past the wrong-content preceding block (this tripwire) AND handles a
        // shrinking edit that shifts the active block to a LOWER offset (the
        // click-switch B-drop, q2-preview-block-nav-p2-5b ":453"). The test.fail() has
        // been removed now that the bug is fixed; see also the negative-shift unit
        // case in outerBlocks-p2-3b.integration.test.ts.

        const serverUrl = getServerUrl();
        const filename = 'self-heal.qmd';

        // Step 1: create project + open file
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: filename, content: FIXTURE_QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, filename);

        // Step 2: snapshot initial file content — hard assertions
        const contentBefore = await getFileContent(page, filename);
        expect(contentBefore).toContain('Third paragraph.');
        expect(contentBefore).not.toContain('COLLABORATOR_INSERT_ABOVE');

        // Step 3: open the MIDDLE (Third paragraph.) block
        const middle = iframe.locator('p[data-block-pool-id]').nth(2);
        await middle.click();
        const ta = iframe.locator('textarea').first();
        await ta.waitFor({ timeout: 5000 });

        // Step 4: SETUP-VALIDITY — confirm the correct block opened and is focused
        await expect(ta).toHaveValue('Third paragraph.');
        expect(
            await iframe.locator('body').evaluate(() => document.activeElement?.tagName === 'TEXTAREA'),
        ).toBe(true);

        // Step 5: fill the textarea with dirty draft
        await ta.fill(DRAFT);
        await expect(ta).toHaveValue(DRAFT);

        // Step 6: inject the collaborator edit (real Automerge write)
        await page.evaluate(async ({ f, c }) => {
            await window.__quartoTestReady;
            window.__quartoTest!.wasmRenderer.updateFileContent(f, c);
        }, { f: filename, c: COLLABORATOR_INSERT_QMD });

        // Step 7: NON-VACUITY guard — prove the re-render happened
        let collaboratorVisible = false;
        for (let i = 0; i < 40; i++) {
            const text = await iframe.locator('body').innerText();
            if (text.includes('COLLABORATOR_INSERT_ABOVE')) {
                collaboratorVisible = true;
                break;
            }
            await page.waitForTimeout(250);
        }
        expect(collaboratorVisible).toBe(true);

        // Step 8: KEEP-1 — editor still open after the shift (self-heal bound)
        await expect(iframe.locator('textarea')).toBeVisible();
        await expect(iframe.locator('textarea')).toHaveCount(1);

        // Step 9: KEEP-2 — draft survived the re-anchor remount (self-heal + controlled-value)
        const ta2 = iframe.locator('textarea').first();
        await expect(ta2).toHaveValue(DRAFT);

        // Step 10: NO-CORRUPTION + SHIFT-OCCURRED
        const contentAfter = await getFileContent(page, filename);
        // Draft was never written to file (only an explicit commit should write)
        expect(contentAfter).not.toContain(DRAFT);
        // Collaborator insert is present
        expect(contentAfter).toContain('COLLABORATOR_INSERT_ABOVE');
        // Active block content is intact in file
        expect(contentAfter).toContain('Third paragraph.');
        // Insert is ABOVE the active block — byte offset shifted (non-vacuity: the
        // active block's textarea genuinely unmounted+re-anchored)
        expect(contentAfter!.indexOf('COLLABORATOR_INSERT_ABOVE')).toBeLessThan(
            contentAfter!.indexOf('Third paragraph.'),
        );
    });
});
