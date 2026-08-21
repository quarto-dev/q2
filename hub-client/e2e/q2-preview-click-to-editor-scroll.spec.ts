/**
 * Preview→editor click-to-editor-scroll: fix verification + one
 * characterization row.
 *
 * Reported symptom: in the HTML preview, clicking in the preview scrolls the
 * Monaco source editor to the corresponding position; in q2-preview, clicking a
 * block to edit it left the editor where it was. Editor→preview worked in both.
 *
 * Root cause, now fixed (T1 pins the browser mechanism the fix works around):
 *   `Q2PreviewIframe` used to drive preview→editor sync off a `click` listener
 *   on the iframe document, but q2-preview activates editing on `pointerup` and
 *   activation REPLACES the clicked element's subtree with the synthetic
 *   `#q2-active-edit-region`. Chromium dispatches no `click` at all when the
 *   `pointerup` target was detached during `pointerup`, so that listener never
 *   ran. The fix switched preview→editor click sync to a capture-phase
 *   `pointerup` listener + `data-loc`, which runs before the DOM swap.
 *
 * Why T1 must live at the real-browser tier: a jsdom test dispatches `click`
 * itself and therefore cannot reproduce the browser's suppression of a click
 * whose target was detached. That fact is invisible below this tier.
 *
 * T1 (characterization) — clicking a block delivers NO `click` event to the
 *                   iframe document (Chromium's detached-target suppression),
 *                   while a click that does not swap the DOM does; AND a
 *                   capture-phase `pointerup` listener on the same document
 *                   DOES see the target still attached with a parseable
 *                   `data-loc` — the fact the fix relies on. This is a
 *                   characterization test, not a repro: it has no production
 *                   revert hunk, because it asserts a browser fact rather than
 *                   this code's behavior. If a future Chromium starts
 *                   delivering that `click`, this test goes red and tells us
 *                   the original `click` listener would have worked after all.
 * T2 (symptom)    — clicking a block does not scroll the editor to that block's
 *                   line. The fixture is crafted so that *ratio* matching (the
 *                   only preview→editor mechanism that exists today) cannot
 *                   make it pass either: a trailing 6000px spacer div puts the
 *                   clicked block at preview scroll ratio ≈ 0.05 while its
 *                   source line is at ≈ 95% of the document. Both are asserted
 *                   as preconditions. Only a `data-loc` based reveal turns T2
 *                   green.
 * T3 (control)    — the same click in the HTML preview DOES reach the iframe
 *                   document, proving the harness and the listener are sound
 *                   and isolating the difference to the DOM swap.
 * T4 (guard)      — a caret-move click inside an already-open editor (not a
 *                   click on a new block) must not move the editor. Passes
 *                   today because nothing reveals yet; its binding is
 *                   post-fix, via the named revert "delete the active-region
 *                   guard from `lineForClickTarget`" (a later task).
 *
 * Run via:
 *   cd hub-client && npx playwright test e2e/q2-preview-click-to-editor-scroll.spec.ts \
 *     --project=chromium --workers=1
 */

import { test, expect, type Page } from '@playwright/test';
import type {} from './helpers/testHooks';
import {
    bootstrapProjectSet,
    createProjectOnServer,
    seedProjectInBrowser,
    getServerUrl,
} from './helpers/projectFactory';
import { waitForPreviewRender } from './helpers/previewExtraction';

const PARA_COUNT = 35;

/**
 * Fixture shape (`format` is spliced in per test):
 *
 *   # Section one           ← source line 5 — gives `SectionizeTransform` a
 *                              section to wrap the body in, so a `[data-loc]`
 *                              lookup from anywhere in the document resolves
 *                              to *something* (the hazard T4 guards against).
 *   Paragraph 1.            ← source line 7
 *   …
 *   Paragraph 35.           ← source line 75 — the click target
 *   ::: {style=6000px}      ← 6000px tall in the preview, 3 lines of source
 *
 * The trailing spacer decouples pixel position from line position: with the
 * preview scrolled just far enough to show `Paragraph 35.` (a few hundred px),
 * the preview's scroll ratio is ≈ 0.05 while that paragraph sits at ≈ 94% of
 * the source. So *ratio* matching — the only preview→editor mechanism that
 * exists today — provably cannot bring line 75 into the editor's viewport;
 * only a `data-loc` based reveal can. Both preconditions are asserted, not
 * assumed.
 */
function fixture(format: string | null): string {
    const paras: string[] = [];
    for (let i = 1; i <= PARA_COUNT; i++) paras.push(`Paragraph ${i}.`, '');
    return [
        '---',
        ...(format ? [`format: ${format}`] : ['title: HTML preview control']),
        '---',
        '',
        '# Section one',
        '',
        ...paras,
        '::: {style="height: 6000px"}',
        'Spacer.',
        ':::',
        '',
    ].join('\n');
}

/** 1-based source line of `Paragraph n.` in the fixture above. */
function paraLine(n: number): number {
    return 5 + 2 * n;
}

const Q2_IFRAME = 'iframe[src*="q2-preview.html"]';
const HTML_IFRAME = 'iframe.preview-active';

async function openDoc(
    page: Page,
    format: string | null,
    kind: 'q2-preview' | 'html',
): Promise<void> {
    const serverUrl = getServerUrl();
    const docId = await createProjectOnServer(serverUrl, [
        { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
        { path: 'doc.qmd', content: fixture(format), contentType: 'text' },
    ]);
    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, docId, serverUrl);
    await page.goto(`/#/p/${localId}/file/doc.qmd`);
    await waitForPreviewRender(page, { kind, timeout: 30000 });
}

/**
 * Install a click-event recorder on the preview iframe's document — the exact
 * event `Q2PreviewIframe` / `MorphIframe` listen for. Same-origin, so the host
 * page can reach `contentDocument` (the q2-preview iframe is sandboxed
 * `allow-same-origin` for precisely this reason).
 */
async function recordIframeDocClicks(page: Page, iframeSelector: string): Promise<void> {
    await page.evaluate((sel) => {
        const frame = document.querySelector(sel) as HTMLIFrameElement | null;
        const doc = frame?.contentDocument;
        if (!doc) throw new Error(`no contentDocument for ${sel}`);
        (window as unknown as { __docClicks: string[] }).__docClicks = [];
        doc.addEventListener('click', (e) => {
            (window as unknown as { __docClicks: string[] }).__docClicks.push(
                (e.target as Element).tagName,
            );
        });
    }, iframeSelector);
}

async function readIframeDocClicks(page: Page): Promise<string[]> {
    return page.evaluate(
        () => (window as unknown as { __docClicks: string[] }).__docClicks,
    );
}

/** One capture-phase `pointerup` observation recorded by {@link recordIframeDocPointerUps}. */
interface PointerUpObservation {
    /** Whether `e.target` was still attached to the document when observed. */
    connected: boolean;
    /** Raw `data-loc` of `e.target`'s nearest `[data-loc]` ancestor (`closest`), or null. */
    dataLoc: string | null;
}

/**
 * Install a CAPTURE-phase `pointerup` recorder on the preview iframe's
 * document — the exact phase and event `Q2PreviewIframe`'s production
 * listener uses (`lineForClickTarget`'s inputs). Unlike
 * {@link recordIframeDocClicks}, this fires before q2-preview's bubble-phase
 * block-activation handler can detach the clicked subtree, so it is expected
 * to see the target still attached — that is the mechanism the fix relies on.
 */
async function recordIframeDocPointerUps(page: Page, iframeSelector: string): Promise<void> {
    await page.evaluate((sel) => {
        const frame = document.querySelector(sel) as HTMLIFrameElement | null;
        const doc = frame?.contentDocument;
        if (!doc) throw new Error(`no contentDocument for ${sel}`);
        (window as unknown as { __docPointerUps: PointerUpObservation[] }).__docPointerUps = [];
        doc.addEventListener(
            'pointerup',
            (e) => {
                const target = e.target as Element;
                (window as unknown as { __docPointerUps: PointerUpObservation[] }).__docPointerUps.push({
                    connected: target.isConnected,
                    dataLoc: target.closest('[data-loc]')?.getAttribute('data-loc') ?? null,
                });
            },
            true,
        );
    }, iframeSelector);
}

async function readIframeDocPointerUps(page: Page): Promise<PointerUpObservation[]> {
    return page.evaluate(
        () => (window as unknown as { __docPointerUps: PointerUpObservation[] }).__docPointerUps,
    );
}

/** Total source lines in the fixture (both formats have the same body). */
const totalLines = fixture('q2-preview').split('\n').length;

/**
 * The preview iframe's scroll ratio, computed exactly as
 * `scrollSyncDom.getIframeScrollRatio` does — this is the number the existing
 * ratio-matching mechanism would feed to the editor.
 */
async function previewScrollRatio(page: Page): Promise<number> {
    return page.evaluate((sel) => {
        const frame = document.querySelector(sel) as HTMLIFrameElement | null;
        const win = frame?.contentWindow;
        const doc = frame?.contentDocument;
        if (!win || !doc) throw new Error('preview iframe not ready');
        const maxScroll = doc.documentElement.scrollHeight - win.innerHeight;
        return maxScroll <= 0 ? 0 : win.scrollY / maxScroll;
    }, 'iframe[src*="q2-preview.html"]');
}

/**
 * The preview iframe's raw `scrollY`, in pixels. Sibling to
 * {@link previewScrollRatio}: used to assert the preview does NOT move (a
 * feedback-loop guard), where a ratio comparison would be too coarse.
 */
async function previewScrollY(page: Page): Promise<number> {
    return page.evaluate((sel) => {
        const frame = document.querySelector(sel) as HTMLIFrameElement | null;
        const win = frame?.contentWindow;
        if (!win) throw new Error('preview iframe not ready');
        return win.scrollY;
    }, Q2_IFRAME);
}

/**
 * Text of the lines Monaco currently has rendered (i.e. what the user sees).
 *
 * **Monaco renders spaces as `\u00a0`.** Without the normalisation below,
 * `innerText.includes('Paragraph 35.')` is `false` even when that line is
 * visibly on screen — which would make every assertion here unsatisfiable
 * (and the `not.toContain(...)` preconditions vacuously true). Verified in
 * Chromium: `view-lines` innerText reports `hasNbsp: true`, and the slice
 * around the match prints identically to a normal space.
 */
async function editorVisibleText(page: Page): Promise<string> {
    const raw = await page.locator('.monaco-editor .view-lines').first().innerText();
    return raw.replace(/\u00a0/g, ' ');
}

test.describe('preview→editor scroll sync on click (repro)', () => {
    test.setTimeout(120000);

    test.beforeEach(async ({ page }, testInfo) => {
        if (testInfo.workerIndex > 0) await page.waitForTimeout(1000);
        await page.addInitScript(() => {
            localStorage.setItem('qh-view-mode', 'both');
            localStorage.setItem('quarto-hub:preferences', JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
            }));
        });
    });

    test('T1 (characterization) — activation delivers no click event, but a capture-phase pointerup sees the attached target', async ({ page }) => {
        await openDoc(page, 'q2-preview', 'q2-preview');
        const iframe = page.frameLocator(Q2_IFRAME);
        await iframe.locator('[data-block-pool-id]').first().waitFor({ timeout: 15_000 });
        await recordIframeDocClicks(page, Q2_IFRAME);
        await recordIframeDocPointerUps(page, Q2_IFRAME);

        // Click a paragraph → activation replaces its subtree with the edit region.
        const target = iframe.locator('p[data-block-pool-id]').filter({ hasText: 'Paragraph 2.' }).first();
        await target.click();
        await iframe.locator('textarea').first().waitFor({ timeout: 5000 });

        const afterActivation = await readIframeDocClicks(page);
        const pointerUps = await readIframeDocPointerUps(page);

        // Control: click again, now INSIDE the open edit region. `onPointerUp`
        // bails on the active-region guard, no DOM swap, so the click survives.
        await iframe.locator('textarea').first().click();
        await page.waitForTimeout(200);
        const afterInRegionClick = await readIframeDocClicks(page);

        expect(
            afterInRegionClick.length,
            'a click that does NOT swap the DOM must reach the iframe document (listener is sound)',
        ).toBeGreaterThan(afterActivation.length);

        // Characterization, not a repro: Chromium really delivers no `click`
        // when the `pointerup` target was detached from the document during
        // `pointerup`. This is the mechanism the fix works around, not a
        // defect this suite is trying to detect.
        expect(
            afterActivation,
            'Chromium delivers no click event when the pointerup target was detached from the document',
        ).toEqual([]);

        // But the capture-phase `pointerup` the fix relies on DOES see the
        // target still attached, with a resolvable `data-loc` — it runs
        // before the bubble-phase activation handler detaches the subtree.
        expect(pointerUps.length, 'expected at least one pointerup for the activation click').toBeGreaterThan(0);
        expect(pointerUps[0].connected, 'capture-phase pointerup must see the target still attached').toBe(true);
        expect(
            pointerUps[0].dataLoc,
            'capture-phase pointerup must see a parseable data-loc',
        ).toMatch(/^\d+:\d+:\d+-\d+:\d+$/);
    });

    test('T2 — clicking a block does not scroll the editor to that block line', async ({ page }) => {
        await openDoc(page, 'q2-preview', 'q2-preview');
        const iframe = page.frameLocator(Q2_IFRAME);
        await iframe.locator('[data-block-pool-id]').first().waitFor({ timeout: 15_000 });

        const targetText = `Paragraph ${PARA_COUNT}.`;
        const target = iframe.locator('p[data-block-pool-id]').filter({ hasText: targetText }).first();

        // Bring the click target into the preview viewport. Thanks to the
        // trailing 6000px spacer this is a small scroll — the preview's ratio
        // stays near 0 while the target's source line is near the end.
        await target.scrollIntoViewIfNeeded();
        await page.waitForTimeout(600); // let any scroll-driven ratio sync settle

        // Precondition A: the fixture really does decouple pixels from lines —
        // the preview's scroll ratio is far below the target's source ratio, so
        // ratio matching cannot satisfy the assertion below.
        const ratio = await previewScrollRatio(page);
        expect(ratio, 'precondition: preview scroll ratio is near the top').toBeLessThan(0.25);
        expect(
            paraLine(PARA_COUNT) / totalLines,
            'precondition: the target line is near the end of the source',
        ).toBeGreaterThan(0.9);

        // Precondition B: the target line is off-screen in the editor.
        const before = await editorVisibleText(page);
        expect(before, 'precondition: the target line is off-screen in the editor').not.toContain(
            targetText,
        );

        const scrollYBeforeClick = await previewScrollY(page);

        await target.click();
        await iframe.locator('textarea').first().waitFor({ timeout: 5000 });
        await page.waitForTimeout(800); // debounce (50ms) + smooth scroll (300ms) + slack

        const after = await editorVisibleText(page);
        expect(
            after,
            'clicking a block should scroll the editor to a line inside that block',
        ).toContain(targetText);

        // No feedback loop: revealing a line in Monaco must not move its
        // cursor in a way that fires onDidChangeCursorPosition, which would
        // feed editor→preview sync and scroll the preview right back.
        const scrollYAfterClick = await previewScrollY(page);
        expect(
            Math.abs(scrollYAfterClick - scrollYBeforeClick),
            'the click must not trigger a reveal→cursor→scroll feedback loop that moves the preview',
        ).toBeLessThanOrEqual(4);
    });

    test('T3 — control: the same click DOES reach the HTML preview document', async ({ page }) => {
        await openDoc(page, null, 'html');
        const iframe = page.frameLocator(HTML_IFRAME);
        await iframe.locator('p').first().waitFor({ timeout: 15_000 });
        await recordIframeDocClicks(page, HTML_IFRAME);

        await iframe.locator('p').filter({ hasText: 'Paragraph 2.' }).first().click();
        await page.waitForTimeout(300);

        expect(
            await readIframeDocClicks(page),
            'HTML preview never swaps its DOM on click, so the click reaches the document',
        ).not.toEqual([]);
    });

    test('T4 — a caret-move click inside the open editor must not move the editor', async ({ page }) => {
        await openDoc(page, 'q2-preview', 'q2-preview');
        const iframe = page.frameLocator(Q2_IFRAME);
        await iframe.locator('[data-block-pool-id]').first().waitFor({ timeout: 15_000 });

        // Open the editor on a LATE paragraph. Once a reveal exists (post-fix)
        // this is what displaces the editor away from `# Section one` — the
        // editor will be showing line ~75, not line 5.
        const targetText = `Paragraph ${PARA_COUNT}.`;
        const target = iframe.locator('p[data-block-pool-id]').filter({ hasText: targetText }).first();
        await target.scrollIntoViewIfNeeded();
        await target.click();
        const textarea = iframe.locator('textarea').first();
        await textarea.waitFor({ timeout: 5000 });
        await page.waitForTimeout(800); // debounce (50ms) + smooth scroll (300ms) + slack, mirrors T2

        const before = await editorVisibleText(page);

        // A caret move WITHIN the block already being edited — not a click on
        // a new block. This must land inside `#q2-active-edit-region`.
        await textarea.click();
        await page.waitForTimeout(300);

        const after = await editorVisibleText(page);
        expect(
            after,
            'a caret-move click inside the already-open editor must not move the editor',
        ).toEqual(before);
    });
});
