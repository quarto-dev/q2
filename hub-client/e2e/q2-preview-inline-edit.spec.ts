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
import {
    activateBlock,
    expectLayoutStable,
    measureLayout,
} from '@quarto/preview-e2e-helpers';

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
    // Wait for the preview iframe to render (the edit round-trip goes through
    // the iframe, not Monaco — Monaco is not required for these tests).
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

    test('activating edit on a heading does not shift the following paragraph (Plan 2c §4)', async ({ page }) => {
        // P1 zero-reflow guarantee: Section 0's EditContentContext keeps the
        // original element (with its CSS margins) in the DOM during editing, so
        // the following sibling must not move when the textarea appears. This
        // exercises the guarantee in a real layout engine — a regression that
        // reverts to bare textarea substitution would drop the heading's margin
        // and shift the paragraph up.
        const serverUrl = getServerUrl();
        const QMD = '---\nformat: q2-preview\n---\n\n## A Heading\n\nFollowing paragraph.\n';
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'reflow.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'reflow.qmd');
        const para = iframe.locator('p[data-block-pool-id]').first();
        await expect(iframe.locator('text=Following paragraph.')).toBeVisible();

        // Measure the paragraph's viewport position before activation.
        const topBefore = await para.evaluate(el => el.getBoundingClientRect().top);

        // Activate edit on the heading (textarea appears inside the wrapped <h2>).
        await iframe.locator('h2[data-block-pool-id]').first().click();
        await iframe.locator('textarea').first().waitFor({ timeout: 5000 });

        // Re-measure; the following paragraph must not have moved (±1px).
        const topAfter = await para.evaluate(el => el.getBoundingClientRect().top);
        expect(Math.abs(topAfter - topBefore)).toBeLessThanOrEqual(1);
    });

    // ── Stronger layout-preservation suite (Plan 2c §4 follow-up) ──────────
    // These assert the FULL zero-reflow invariant: activating any block moves
    // no other block and does not change the document height. They catch the
    // "space crunch" the single-paragraph test above misses — e.g. a heading
    // losing its Bootstrap padding-bottom + border-bottom (the rule under h2),
    // or the gap between a paragraph and a following list collapsing.

    test('activating a heading preserves all surrounding layout (no crunch)', async ({ page }) => {
        const serverUrl = getServerUrl();
        const QMD =
            '---\nformat: q2-preview\n---\n\nIntro paragraph.\n\n## A Heading\n\nBody paragraph one.\n\nBody paragraph two.\n';
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'crunch-heading.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'crunch-heading.qmd');
        await expect(iframe.locator('text=Body paragraph one.')).toBeVisible();

        // Capture the heading's bottom rule (Bootstrap border-bottom) before edit.
        const ruleBefore = await iframe
            .locator('h2[data-block-pool-id]')
            .first()
            .evaluate(el => {
                const cs = getComputedStyle(el);
                return {
                    width: cs.borderBottomWidth,
                    style: cs.borderBottomStyle,
                    color: cs.borderBottomColor,
                };
            });
        // Sanity: this fixture's heading actually HAS a visible rule, otherwise
        // the persistence check below would be vacuous.
        expect(ruleBefore.style, 'fixture heading must have a border-bottom rule').not.toBe('none');
        expect(parseFloat(ruleBefore.width)).toBeGreaterThan(0);

        const before = await measureLayout(iframe);
        const edited = await activateBlock(iframe, 'h2');
        const after = await measureLayout(iframe);
        expectLayoutStable(before, after, edited);

        // The rule must NOT disappear when editing: the edit wrapper (the
        // textarea's parent <div>) reproduces the same border-bottom.
        const ruleAfter = await iframe
            .locator('textarea')
            .first()
            .evaluate(el => {
                const cs = getComputedStyle(el.parentElement as HTMLElement);
                return {
                    width: cs.borderBottomWidth,
                    style: cs.borderBottomStyle,
                    color: cs.borderBottomColor,
                };
            });
        expect(ruleAfter, 'h2 bottom rule must persist while editing').toEqual(ruleBefore);
    });

    test('activating a paragraph before a list preserves the list position', async ({ page }) => {
        const serverUrl = getServerUrl();
        const QMD =
            '---\nformat: q2-preview\n---\n\nLead paragraph.\n\n- first item\n- second item\n- third item\n\nTrailing paragraph.\n';
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'crunch-list.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'crunch-list.qmd');
        await expect(iframe.locator('text=Lead paragraph.')).toBeVisible();

        // Edit the lead paragraph — the list and trailing paragraph below must not move.
        const before = await measureLayout(iframe);
        const edited = await activateBlock(iframe, 'p');
        const after = await measureLayout(iframe);
        expectLayoutStable(before, after, edited);
    });

    test('activating a list preserves the trailing paragraph position', async ({ page }) => {
        const serverUrl = getServerUrl();
        const QMD =
            '---\nformat: q2-preview\n---\n\nLead paragraph.\n\n- first item\n- second item\n\nTrailing paragraph.\n';
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'crunch-list2.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'crunch-list2.qmd');
        await expect(iframe.locator('text=Trailing paragraph.')).toBeVisible();

        // Reference: the lead paragraph's left edge = the document text column.
        const paraLeft = await iframe
            .locator('p[data-block-pool-id]')
            .first()
            .evaluate(el => el.getBoundingClientRect().left);

        const before = await measureLayout(iframe);
        const edited = await activateBlock(iframe, 'ul');
        const after = await measureLayout(iframe);
        expectLayoutStable(before, after, edited);

        // The list's editing textarea must start at the text column, NOT be
        // pushed right by the bullet gutter (the <ul> padding-left).
        const taLeft = await iframe
            .locator('textarea')
            .first()
            .evaluate(el => el.getBoundingClientRect().left);
        expect(
            Math.abs(taLeft - paraLeft),
            `list textarea is indented ${(taLeft - paraLeft).toFixed(1)}px past the text column`,
        ).toBeLessThanOrEqual(1.5);
    });

    test('editing a paragraph inside a fenced div updates only that block (Plan 3)', async ({ page }) => {
        const serverUrl = getServerUrl();
        const QMD = [
            '---',
            'format: q2-preview',
            '---',
            '',
            'Outer paragraph.',
            '',
            '::: {.edit-test-div}',
            'Inner paragraph.',
            ':::',
            '',
        ].join('\n');
        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'nested.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'nested.qmd');
        await expect(iframe.locator('text=Inner paragraph.')).toBeVisible();

        // Target the Para *inside* the fenced div — a Descendable block.
        // After Plan 3 the widened gate gives it data-block-pool-id; clicking
        // it activates the textarea via the Block dispatcher.
        await iframe.locator('div.edit-test-div p[data-block-pool-id]').first().click();
        const ta = iframe.locator('textarea').first();
        await ta.waitFor({ timeout: 5000 });
        await ta.fill('Edited inner paragraph.');
        await ta.press('Control+Enter');

        await assertAutomerge(page, 'nested.qmd', {
            contains: ['Edited inner paragraph.', 'Outer paragraph.'],
            lacks: ['Inner paragraph.'],
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
