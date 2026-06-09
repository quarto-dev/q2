/**
 * E2E tests for the reactji / comment render-component in q2-preview.
 *
 * comment.tsx uses Plan 2b's `commitSubtreeEdit` (via `usePreviewEdit`)
 * so add/remove reactions round-trip through `apply_node_edit` → VFS →
 * Automerge → re-render.  All three tests verify that the QMD actually
 * changed (`assertAutomerge`) in addition to the DOM change.
 *
 * Test layout:
 *   1. "attribution surface + actor reach user TSX" — plumbing smoke test
 *      that the Phase 2a/2b hooks are on the renderer surface and the
 *      injected actor id travels end-to-end.
 *   2. "adding a reaction persists through the round-trip" — pick a new
 *      emoji via the picker; verify bubble appears and QMD gains the span.
 *   3. "removing a reaction persists through the round-trip" — add an
 *      emoji, wait for re-render, click the bubble to remove; verify
 *      bubble disappears and QMD loses the span.
 *      Remove-mine works via the `mySessionReactions` ref in comment.tsx:
 *      attribution-based removal (Phase 2c) fires when Attribution is on;
 *      the ref-based fallback fires in single-actor e2e where the toggle
 *      is off.
 *
 * Note on `assertAutomerge`: polls `wasmRenderer.getFileContent()` (the
 * Automerge-backed VFS layer) rather than asserting on DOM text, because
 * DOM text is not reliable for edited contentEditable regions and isn't
 * the right layer to prove the QMD actually changed.
 */

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { test, expect, type Page, type FrameLocator, type ConsoleMessage } from '@playwright/test';
import {
    bootstrapProjectSet,
    createProjectOnServer,
    seedProjectInBrowser,
    getServerUrl,
} from './helpers/projectFactory';
import { waitForPreviewRender } from './helpers/previewExtraction';

const FIXTURE_DIR = resolve(
    import.meta.dirname,
    '../../crates/quarto/tests/playwright-fixtures/q2-preview/render-components-comment',
);

const qmdContent = readFileSync(resolve(FIXTURE_DIR, 'render-components-comment.qmd'), 'utf-8');
const tsxContent = readFileSync(resolve(FIXTURE_DIR, 'comment.tsx'), 'utf-8');
const quartoYmlContent = readFileSync(resolve(FIXTURE_DIR, '_quarto.yml'), 'utf-8');

// Stable test actor id injected via __QUARTO_TEST_ACTOR_ID__ (App.tsx
// reads this when VITE_E2E is set).  Must be 32 hex chars so
// Automerge accepts it as a valid actor id.
const TEST_ACTOR_ID = 'e2e7e1f02a30000000000000000007e1';

// ── Helpers ──────────────────────────────────────────────────────────

/** Seed a fresh project, navigate to the qmd, and wait for the first render. */
async function openCommentFixture(page: Page): Promise<FrameLocator> {
    const serverUrl = getServerUrl();
    const indexDocId = await createProjectOnServer(serverUrl, [
        { path: '_quarto.yml', content: quartoYmlContent, contentType: 'text' },
        { path: 'comment.tsx', content: tsxContent, contentType: 'text' },
        { path: 'render-components-comment.qmd', content: qmdContent, contentType: 'text' },
    ]);
    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);
    await page.goto(`/#/p/${localId}/file/${encodeURIComponent('render-components-comment.qmd')}`);
    await waitForPreviewRender(page, { kind: 'q2-preview', timeout: 30_000 });
    const iframe = page.frameLocator('iframe[src*="q2-preview.html"]');
    // comment.tsx renders a "+ 🙂" picker button on every wrapped block
    await expect(iframe.locator('[title="Add reaction"]').first()).toBeVisible({ timeout: 30_000 });
    return iframe;
}

/** Poll the Automerge VFS until the file's content satisfies all checks. */
async function assertAutomerge(
    page: Page,
    filename: string,
    { contains = [], lacks = [] }: { contains?: string[]; lacks?: string[] },
): Promise<void> {
    await expect(async () => {
        const text = await page.evaluate(async (f) => {
            await (window as any).__quartoTestReady;
            return (window as any).__quartoTest?.wasmRenderer.getFileContent(f) as string | null;
        }, filename);
        expect(text, `getFileContent(${filename}) must return a string`).not.toBeNull();
        for (const s of contains) expect(text, `QMD must contain "${s}"`).toContain(s);
        for (const s of lacks) expect(text, `QMD must not contain "${s}"`).not.toContain(s);
    }).toPass({ timeout: 15_000 });
}

// ── Tests ─────────────────────────────────────────────────────────────

test.describe('q2-preview render-components-comment', () => {
    test.setTimeout(120_000);

    test.beforeEach(async ({ page }, testInfo) => {
        // Inject actor id before any app script runs.
        await page.addInitScript((id) => {
            (window as any).__QUARTO_TEST_ACTOR_ID__ = id;
        }, TEST_ACTOR_ID);
        // Stagger parallel workers to avoid Monaco AMD init race.
        if (testInfo.workerIndex > 0) await page.waitForTimeout(1000);
    });

    test('attribution surface + current actor reach user TSX', async ({ page }) => {
        const consoleAll: string[] = [];
        page.on('console', (m: ConsoleMessage) => consoleAll.push(`[${m.type()}] ${m.text()}`));
        const pageErrors: string[] = [];
        page.on('pageerror', (e) => pageErrors.push(e.message));

        const iframe = await openCommentFixture(page);

        // Fixture renders aggregated bubbles for existing reactions.
        await expect(iframe.locator('[title="Add 🤔"]').first()).toBeVisible();
        await expect(iframe.locator('[title="Add 🔥"]').first()).toBeVisible();

        // No placeholder leakage — comment.tsx Block override must cover
        // every block type in the fixture.
        await expect(iframe.locator('div.q2-preview-placeholder')).toHaveCount(0);

        // Wait for the Diagnostic sub-component's useEffect to run
        // (only mounts when both attribution hooks are present).
        await page.waitForTimeout(500);

        const diag = await iframe.locator('body').evaluate(() => (window as any).__COMMENT_DIAG__);
        console.log('COMMENT_DIAG =', JSON.stringify(diag, null, 2));

        expect
            .soft(diag?.hasUseNodeAttribution, 'useNodeAttribution must be on __Q2_PREVIEW_RENDERER__')
            .toBe(true);
        expect
            .soft(diag?.hasUseCurrentActor, 'useCurrentActor must be on __Q2_PREVIEW_RENDERER__')
            .toBe(true);
        expect
            .soft(diag?.me, `actor id must round-trip to the iframe (got ${JSON.stringify(diag?.me)})`)
            .toBe(TEST_ACTOR_ID);
        expect(diag, 'COMMENT_DIAG must be populated').toBeTruthy();
    });

    test('adding a reaction persists through the round-trip', async ({ page }) => {
        const iframe = await openCommentFixture(page);

        // Use the picker on the first wrapped block to add a fresh emoji
        // that is NOT in the fixture initially.  👍 does not appear in
        // render-components-comment.qmd at all, so count starts at 0.
        const picker = iframe.locator('[title="Add reaction"]').first();
        await picker.click();
        const thumbsUpButton = iframe.locator('span').filter({ hasText: '👍' }).first();
        await thumbsUpButton.click();

        // DOM: the 👍 bubble should appear with count 1.
        const thumbsBubble = iframe.locator('[title="Add 👍"]').first();
        await expect(thumbsBubble).toBeVisible({ timeout: 15_000 });
        await expect(thumbsBubble).toContainText('1');

        // Automerge layer: the QMD must contain the reaction span.
        // apply_node_edit writes Span nodes as [emoji]{.quarto-edit-comment}.
        await assertAutomerge(page, 'render-components-comment.qmd', {
            contains: ['👍'],
        });
    });

    test('removing a reaction persists through the round-trip', async ({ page }) => {
        const iframe = await openCommentFixture(page);

        // Step 1: add 👍 (same as the "add" test above).
        const picker = iframe.locator('[title="Add reaction"]').first();
        await picker.click();
        const thumbsUpButton = iframe.locator('span').filter({ hasText: '👍' }).first();
        await thumbsUpButton.click();

        // Wait for the add to round-trip (bubble becomes visible).
        const thumbsBubble = iframe.locator('[title="Add 👍"]').first();
        await expect(thumbsBubble).toBeVisible({ timeout: 15_000 });
        await expect(thumbsBubble).toContainText('1');

        // Step 2: click the bubble to remove it.
        // comment.tsx sees `mySessionReactions.get('👍') === 1` and calls
        // removeFirstMatchingInSource → commitSubtreeEdit → round-trip.
        await thumbsBubble.click();

        // DOM: the 👍 bubble should disappear (count 0 → no bubble rendered).
        await expect(thumbsBubble).not.toBeVisible({ timeout: 15_000 });

        // Automerge layer: the QMD must no longer contain the reaction span.
        await assertAutomerge(page, 'render-components-comment.qmd', {
            lacks: ['👍'],
        });
    });
});
