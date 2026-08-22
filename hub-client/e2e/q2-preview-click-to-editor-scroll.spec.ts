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
 *                   click on a new block) must not move the editor. Uses its
 *                   own `wrapperFixture()` (task 11, 2026-08-21 plan):
 *                   the edited block sits inside a located non-section
 *                   wrapper (a plain fenced div — NOT a callout; see that
 *                   fixture's doc comment for why callouts don't work here),
 *                   so a deleted active-region guard resolves to the
 *                   wrapper's own line instead of doing nothing. Named
 *                   revert: delete the active-region guard from
 *                   `lineForClickTarget` → RED (the editor jumps to the
 *                   wrapper's line).
 * A1g (2026-08-22 plan) — click-to-ALIGN, not just reveal: the clicked
 *                   block's on-screen y (before the click) and the target
 *                   line's rendered y in Monaco (after the click settles)
 *                   must agree within a stated tolerance. This is the only
 *                   row in the suite that binds the two coordinate spaces
 *                   (iframe viewport vs. host page) — jsdom has no layout,
 *                   so nothing below this tier can catch a wrong iframe
 *                   offset; see that row's own comment for the tolerance
 *                   and why.
 * P2a (2026-08-22 plan, Phase 2) — the HTML preview's own click-to-ALIGN
 *                   (T3's control proves the HTML preview receives clicks
 *                   at all; this proves what it does with them). Uses a
 *                   dedicated fixture, `htmlAlignFixture()`, whose target
 *                   paragraph wraps across several visual rows — the only
 *                   way to prove `hostY` comes from the clicked SPAN, not
 *                   the containing block, since a single-line paragraph
 *                   (as in `fixture()`) can't tell the two apart.
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

/**
 * T5's own fixture — self-contained (no `.scratch/` dependency), confirmed
 * by direct probing against a real hub server (see task-9-report.md) to
 * reproduce a genuine browser `scroll` event on the preview when the FIRST
 * callout's body is clicked, and — unguarded — a real overwrite of the
 * editor's reveal a few ms later.
 *
 * This is deliberately NOT `fixture()`. Probing found the reflow-then-scroll
 * is NOT produced by a callout in isolation (`fixture()`'s heading + one
 * short numbered paragraph + a callout produces no `scroll` event at all —
 * confirmed with direct instrumentation, several variations tried). It IS
 * reliably produced by this shape: a full title block (title + subtitle +
 * author — a bare `format:` front-matter was NOT enough) followed by a
 * multi-paragraph intro, mirroring the structure of
 * `.scratch/demo/scroll-sync-demo.qmd` (where task-8 first found the race)
 * without depending on that file. The exact mechanism ("why" the toolbar
 * mount reflows here and not in a barer document) was not pinned down further
 * — see task-9-report.md for the elimination steps — but the reproduction
 * itself was confirmed twice, deterministically.
 *
 * Layout, mirroring `fixture()`'s own T2 trick for the same reason:
 *   - 40 filler paragraphs before "## Callouts" — without them the callout
 *     sits within Monaco's initial render window and there is nothing for a
 *     reveal (or an overwrite) to do; the "off-screen" precondition needs
 *     real distance.
 *   - a trailing 6000px spacer AFTER the callouts — decouples the target
 *     callout's low PREVIEW pixel-ratio from its high source LINE-ratio, the
 *     same way `fixture()`'s spacer does for `Paragraph N.`. Without it, the
 *     pre-existing ratio-sync's guess is usually close enough to the correct
 *     line to still (accidentally) satisfy a "still contains the target"
 *     assertion — confirmed by probing: the same click, same reflow, same
 *     `scroll` event, but a much smaller (~8-line) and inconsequential drift
 *     without the spacer, vs. a large, target-excluding drift with it.
 */
function reflowFixture(): string {
    const filler: string[] = [];
    for (let i = 1; i <= 40; i++) filler.push(`Paragraph ${i}.`, '');
    return [
        '---',
        'title: "Click-to-Editor Scroll Sync — Element Gallery"',
        'subtitle: "A document for exercising preview → editor scroll sync"',
        'author: "Quarto 2"',
        'format: q2-preview',
        '---',
        '',
        '# Introduction',
        '',
        'This document exists to exercise **click-to-editor scroll sync** in the',
        'q2-preview. Clicking any block below should scroll the Monaco source editor to',
        "that block's line, *without* stealing focus from the inline editor the same",
        'click opens.',
        '',
        'It is deliberately long and deliberately varied: every block kind is a',
        'different shape in the DOM, and the interesting question is whether',
        '`lineForClickTarget` resolves each one to the right `data-loc`.',
        '',
        'Try clicking a paragraph near the bottom first — the editor should jump to a',
        'line in the nineties, not to the top of the file.',
        '',
        ...filler,
        '## Callouts',
        '',
        '::: {.callout-note}',
        '## A note callout',
        '',
        'Callouts nest their content inside a div, so the nearest data-loc ancestor',
        'of this paragraph is the paragraph — not the callout wrapper.',
        ':::',
        '',
        '::: {.callout-tip}',
        '## A tip callout',
        '',
        'Clicking the heading of a callout is a different DOM path from clicking its',
        'body. Both should resolve.',
        ':::',
        '',
        '::: {.callout-warning}',
        '## A warning callout',
        '',
        'Warnings, notes, tips, cautions and importants all render through the same',
        'machinery but with different classes.',
        ':::',
        '',
        '::: {.callout-important}',
        '## An important callout',
        '',
        'This one exists mostly to add height so the document needs more scrolling.',
        ':::',
        '',
        '::: {.callout-caution collapse="true"}',
        '## A collapsed caution callout',
        '',
        'Collapsed callouts start closed, which means their body is in the DOM but not',
        'visible — a useful edge case for hit-testing.',
        ':::',
        '',
        '::: {style="height: 6000px"}',
        'Spacer.',
        ':::',
        '',
    ].join('\n');
}

/**
 * T4's own fixture (task 11, 2026-08-21 plan) — a plain fenced div
 * (`.plain-wrapper`), NOT a callout, wrapping a single paragraph, after 40
 * filler paragraphs so the wrapper starts off-screen at load.
 *
 * Why not a callout (reflowFixture()'s existing wrapper): inspecting the real
 * q2-preview DOM (via the ancestor chain from `#q2-active-edit-region` after
 * opening a callout's body paragraph) shows the callout's own `<div>` never
 * carries `data-loc` — `Callout.tsx`'s dispatcher spreads `affordanceAttr`
 * onto its wrapper but never `dataLocProps`, unlike the generic Div
 * dispatcher (`Div.tsx`, used for a plain `::: {...}` fenced div), which
 * does. So with a callout, `closest('[data-loc]')` from inside the region
 * walks all the way past the callout AND the enclosing `<section>` (which
 * also carries no `data-loc` in this renderer) to nothing — it returns null
 * via the "no ancestor at all" case regardless of whether the active-region
 * guard exists, exactly the vacuous-row failure task 11 was dispatched to
 * fix. A plain fenced div's outer element DOES carry its own `data-loc`
 * (confirmed empirically), so it is the wrapper this row actually needs.
 *
 * Why the test also has to scroll Monaco directly (see the test body): the
 * wrapper's own `data-loc` start line and its single paragraph child's start
 * line differ by only 1-3 lines (the fence line itself, maybe a heading).
 * Opening the paragraph's editor reveals its own line correctly either way,
 * but `revealEditorLine` calls `revealLineInCenterIfOutsideViewport` — since
 * the wrapper's line is already inside that same small-gap viewport, a
 * missing guard's reveal call would silently no-op too, and the row would be
 * vacuous again for a completely different reason. The test scrolls Monaco
 * away from both lines (a real user could do this with the mouse wheel,
 * without touching the preview or closing the open editor) so the wrapper's
 * line is genuinely outside the viewport at the moment of the final click.
 */
function wrapperFixture(): string {
    const filler: string[] = [];
    for (let i = 1; i <= 40; i++) filler.push(`Paragraph ${i}.`, '');
    return [
        '---',
        'title: "T4 — active-region guard, non-section wrapper"',
        'format: q2-preview',
        '---',
        '',
        '# Introduction',
        '',
        ...filler,
        '::: {.plain-wrapper}',
        'Wrapped paragraph for the T4 active-region guard row.',
        ':::',
        '',
    ].join('\n');
}

/**
 * P2a's own fixture (2026-08-22 plan, Phase 2) — a single long paragraph,
 * all on ONE source line, with enough words to wrap across several visual
 * lines in the HTML preview at the default viewport width.
 *
 * The wrap is the whole point: the HTML writer stamps `data-loc` on
 * inlines, so each word gets its own `<span data-loc>`, all sharing the
 * SAME source line (there is only one line of source). On `fixture()`'s
 * own one-line-per-paragraph text this would be indistinguishable from
 * measuring the containing `<p>` — a single-line block's own top coincides
 * with any of its words' tops. Only a WRAPPED paragraph separates the two:
 * a word near the end renders several line-heights below the block's own
 * top, so a wrong implementation that measured the `<p>` instead of the
 * clicked span would land the editor at the wrong on-screen y by a
 * detectable amount, not just be off by the sub-pixel noise A1g's
 * tolerance already absorbs.
 */
function htmlAlignFixture(): string {
    // Filler before AND after the target paragraph, mirroring wrapperFixture()'s
    // reasoning: enough total content that the editor actually has scroll
    // range (with only the target paragraph, `getScrollHeight() -
    // getLayoutInfo().height` could be ≤ 0, and revealEditorLine's clamp would
    // force scrollTop to 0 regardless of hostY — vacuous, same trap A1g's own
    // "paragraph 20 of 80" choice avoids), and positioned away from either end
    // so the clamp can't coincidentally mask a wrong computation.
    const filler: string[] = [];
    for (let i = 1; i <= 30; i++) filler.push(`Filler paragraph ${i}.`, '');
    const words: string[] = [];
    for (let i = 1; i <= 40; i++) words.push(`word${i}`);
    return [
        '---',
        'title: "P2a — HTML preview alignment anchors on the clicked span, not the block"',
        '---',
        '',
        ...filler,
        words.join(' ') + '.',
        '',
        ...filler,
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
 * Like {@link openDoc}, but for a dedicated fixture's content (T5's
 * `reflowFixture()`, T4's `wrapperFixture()`).
 */
async function openDocWithContent(page: Page, content: string): Promise<void> {
    const serverUrl = getServerUrl();
    const docId = await createProjectOnServer(serverUrl, [
        { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
        { path: 'doc.qmd', content, contentType: 'text' },
    ]);
    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, docId, serverUrl);
    await page.goto(`/#/p/${localId}/file/doc.qmd`);
    await waitForPreviewRender(page, { kind: 'q2-preview', timeout: 30000 });
}

/**
 * Like {@link openDocWithContent}, but waits on the HTML preview's own
 * readiness signal instead of q2-preview's (P2a's `htmlAlignFixture()`).
 */
async function openHtmlDocWithContent(page: Page, content: string): Promise<void> {
    const serverUrl = getServerUrl();
    const docId = await createProjectOnServer(serverUrl, [
        { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
        { path: 'doc.qmd', content, contentType: 'text' },
    ]);
    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, docId, serverUrl);
    await page.goto(`/#/p/${localId}/file/doc.qmd`);
    await waitForPreviewRender(page, { kind: 'html', timeout: 30000 });
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
 * Install a `scroll` counter on the preview iframe's `contentWindow` — the
 * same event `Q2PreviewIframe`'s production listener (and `handlePreviewScroll`
 * → the ratio-sync debounce) reacts to. Modeled on {@link recordIframeDocClicks}
 * (install-then-read-back via `page.evaluate`), not a new shape. T5 uses this
 * to confirm a real scroll actually fired, rather than assuming it did.
 */
async function recordIframePreviewScrolls(page: Page, iframeSelector: string): Promise<void> {
    await page.evaluate((sel) => {
        const frame = document.querySelector(sel) as HTMLIFrameElement | null;
        const win = frame?.contentWindow;
        if (!win) throw new Error(`no contentWindow for ${sel}`);
        (window as unknown as { __previewScrollCount: number }).__previewScrollCount = 0;
        win.addEventListener(
            'scroll',
            () => {
                (window as unknown as { __previewScrollCount: number }).__previewScrollCount += 1;
            },
            { passive: true },
        );
    }, iframeSelector);
}

async function readIframePreviewScrollCount(page: Page): Promise<number> {
    return page.evaluate(
        () => (window as unknown as { __previewScrollCount: number }).__previewScrollCount,
    );
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

test.describe('preview→editor scroll sync on click', () => {
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
        // Own fixture (`wrapperFixture()`) — see its doc comment for why a
        // plain fenced div, not one of reflowFixture()'s callouts, is the
        // wrapper this row needs, and why the block being edited must sit
        // inside it (not directly inside the section).
        await openDocWithContent(page, wrapperFixture());
        const iframe = page.frameLocator(Q2_IFRAME);
        await iframe.locator('[data-block-pool-id]').first().waitFor({ timeout: 15_000 });

        // 1. Click a block deep in the document (far from the wrapper, which
        // sits after 40 filler paragraphs) to displace the editor there.
        const deep = iframe.locator('p[data-block-pool-id]').filter({ hasText: 'Paragraph 1.' }).first();
        await deep.scrollIntoViewIfNeeded();
        await deep.click();
        const textarea = iframe.locator('textarea').first();
        await textarea.waitFor({ timeout: 5000 });
        await page.waitForTimeout(800); // debounce (50ms) + smooth scroll (300ms) + slack, mirrors T2

        // 2. Open the wrapper-nested block's editor. Correctly reveals its
        // own line (this happens whether or not the active-region guard
        // exists — the click target isn't inside the region yet).
        const target = iframe.getByText(/^Wrapped paragraph for the T4 active-region guard row/).first();
        await target.scrollIntoViewIfNeeded();
        await target.click();
        await textarea.waitFor({ timeout: 5000 });
        await page.waitForTimeout(800);

        // 3. Displace Monaco's OWN viewport again, directly — a real user
        // action (mouse wheel over the editor pane) that does not touch the
        // preview and so cannot close the edit region just opened. Needed
        // per wrapperFixture()'s doc comment: the wrapper's line is only a
        // few lines from the paragraph's own line, so without this the
        // wrapper's line would already be inside the viewport and a missing
        // guard's reveal call would no-op too, same as a correct guard would.
        await page.hover('.monaco-editor');
        for (let i = 0; i < 40; i++) await page.mouse.wheel(0, -2000);
        await page.waitForTimeout(300);

        const before = await editorVisibleText(page);

        // 4. A caret move WITHIN the block already being edited — not a
        // click on a new block. This must land inside `#q2-active-edit-region`.
        await textarea.click();
        await page.waitForTimeout(300);

        const after = await editorVisibleText(page);
        expect(
            after,
            'a caret-move click inside the already-open editor must not move the editor',
        ).toEqual(before);
    });

    test('T5 — a reflow-causing rich-text activation must not let the reveal be overwritten by ratio sync', async ({ page }) => {
        // richText defaults OFF for this describe's shared preferences seed
        // (bootstrapProjectSet's e2e baseline), so T1–T4's plain-textarea
        // assertions are unaffected. This test opts IN, registering its own
        // addInitScript AFTER the shared beforeEach's — it runs later, so it
        // wins, and bootstrapProjectSet's merge (`cur.richText === undefined`)
        // then leaves it alone. Mirrors q2-preview-richtext-bold-content-loss.spec.ts.
        await page.addInitScript(() => {
            localStorage.setItem(
                'quarto-hub:preferences',
                JSON.stringify({
                    version: 1,
                    scrollSyncEnabled: true,
                    errorOverlayCollapsed: true,
                    colorScheme: 'auto',
                    richText: true,
                }),
            );
        });

        await openDocWithContent(page, reflowFixture());
        const iframe = page.frameLocator(Q2_IFRAME);
        await iframe.locator('[data-block-pool-id]').first().waitFor({ timeout: 15_000 });

        const targetText = 'Callouts nest their content inside a div';
        const target = iframe.getByText(new RegExp(`^${targetText}`)).first();

        // Precondition: the target is genuinely off-screen in the editor
        // before any interaction — see reflowFixture()'s doc comment for why
        // the 40 filler paragraphs are needed for this to hold.
        const before = await editorVisibleText(page);
        expect(before, 'precondition: the target line is off-screen in the editor').not.toContain(
            targetText,
        );

        await target.scrollIntoViewIfNeeded();

        // Install the scroll counter AFTER the setup scroll above (so it
        // only counts scrolls the CLICK itself produces, not the one from
        // bringing the target into view) and BEFORE the click.
        await recordIframePreviewScrolls(page, Q2_IFRAME);

        await target.click();
        // Activation opens the rich-text (tiptap) editor for this block —
        // `.ProseMirror`, not a `<textarea>` — confirming the click landed
        // and the block is now in rich-edit mode.
        await iframe.locator('.ProseMirror').first().waitFor({ timeout: 5000 });

        // Runtime precondition, not assumed: activation must have produced at
        // least one real `scroll` event on the preview. Without this, the
        // poll loop below would pass VACUOUSLY if `reflowFixture()` ever
        // stops reproducing the post-activation reflow (a dependency bump, a
        // CSS change, a font-loading change, ...) — nothing would move the
        // reveal, so "still contains target text" would hold trivially. A
        // failure here means this row's fixture precondition broke, not that
        // the fix regressed — re-derive `reflowFixture()` (see its doc
        // comment), don't delete this row.
        const scrollCount = await readIframePreviewScrollCount(page);
        expect(
            scrollCount,
            'precondition: activation must produce at least one real preview scroll event — ' +
                'if this is 0, reflowFixture() has stopped reproducing the reflow this row exists ' +
                'to survive; re-derive the fixture, this is not a product regression',
        ).toBeGreaterThan(0);

        // Poll across (and past) the ratio-sync debounce window (50ms) plus
        // its own smooth-scroll animation (300ms). A single post-click check
        // would land right where the existing T2 does today and miss a
        // reveal that arrives correctly, then is silently overwritten a few
        // ms later — which is exactly what task-8 found (and task-9-report.md
        // reproduces directly against this fixture) and this row exists to
        // catch. Once overwritten, the wrong position is not self-correcting
        // (nothing else scrolls the editor again), so any single sample in
        // this window catches a regression — the loop's job is to not miss a
        // narrow window by sampling too coarsely.
        for (let elapsed = 0; elapsed <= 500; elapsed += 25) {
            const visible = await editorVisibleText(page);
            expect(
                visible,
                `the reveal must still hold ${elapsed}ms after the click, not just immediately after it`,
            ).toContain(targetText);
            await page.waitForTimeout(25);
        }
    });

    test('A1g — clicking a block aligns its source line to the same on-screen y, not merely reveals it', async ({ page }) => {
        // Paragraph 20 sits at source line 45 of 80 (paraLine(20)), safely away
        // from the document's start or end — the alignment clamp (plan's "near
        // the start or end… the clamp wins") would otherwise mask a wrong
        // unclamped computation by coincidentally landing at the same clamped
        // bound regardless of hostY.
        await openDoc(page, 'q2-preview', 'q2-preview');
        const iframe = page.frameLocator(Q2_IFRAME);
        await iframe.locator('[data-block-pool-id]').first().waitFor({ timeout: 15_000 });

        const targetText = 'Paragraph 20.';
        const target = iframe.locator('p[data-block-pool-id]').filter({ hasText: targetText }).first();
        await target.scrollIntoViewIfNeeded();
        await page.waitForTimeout(300); // let the scrollIntoView settle before measuring

        // hostY: the clicked block's top edge in HOST-PAGE coordinates — exactly
        // what Q2PreviewIframe's pointerup handler computes and passes as
        // `onClickAtLine`'s second argument. Playwright's boundingBox() is
        // already reported relative to the main frame's viewport (not the
        // iframe's), so this is the same coordinate space without redoing the
        // iframe-offset arithmetic by hand.
        const blockBoxBeforeClick = await target.boundingBox();
        expect(blockBoxBeforeClick, 'precondition: the clicked block must be measurable').not.toBeNull();
        const hostY = blockBoxBeforeClick!.y;

        await target.click();
        await iframe.locator('textarea').first().waitFor({ timeout: 5000 });
        // Debounce is not in play here (revealEditorLine is not debounced —
        // U2d), but Monaco's own ScrollType.Smooth animation and the
        // isSyncingRef suppression window both run for up to 300ms.
        await page.waitForTimeout(600);

        // The target line's rendered y in Monaco, in the same host-page
        // coordinate space (Monaco lives directly in the host page, not in an
        // iframe, so no offset arithmetic is needed here).
        const monacoLine = page
            .locator('.monaco-editor .view-lines .view-line')
            .filter({ hasText: targetText })
            .first();
        await monacoLine.waitFor({ timeout: 5000 });
        const lineBox = await monacoLine.boundingBox();
        expect(lineBox, 'the target line must be rendered (in the DOM) after alignment').not.toBeNull();

        // Tolerance: 6px. The computation is exact integer arithmetic
        // (getTopForLineNumber - (hostY - editorTop)), but two independent
        // boundingBox() reads and Monaco's own sub-pixel line positioning can
        // each be off by a fraction of a pixel; 6px comfortably absorbs that
        // without masking a real defect. A wrong iframe-coordinate-space
        // computation (the bug this row exists to catch — see the plan's "two
        // coordinate spaces" note) is off by a whole constant offset: the
        // height of whatever chrome sits above the preview pane (tens to
        // hundreds of px), an order of magnitude past this tolerance.
        const ALIGNMENT_TOLERANCE_PX = 6;
        expect(
            Math.abs(lineBox!.y - hostY),
            `clicked block's on-screen y (${hostY}) and the target line's rendered y in Monaco (${lineBox!.y}) must agree within ${ALIGNMENT_TOLERANCE_PX}px`,
        ).toBeLessThanOrEqual(ALIGNMENT_TOLERANCE_PX);
    });

    test('P2a — clicking a word in the HTML preview aligns it to the same on-screen y as that SPECIFIC word, not the containing block', async ({ page }) => {
        // htmlAlignFixture()'s target paragraph wraps across several visual
        // rows in the preview; word40 (its last word) renders well below the
        // paragraph's own <p> top. hostY is measured from word40 itself — the
        // only way to discriminate "hostY comes from the clicked SPAN" (this
        // phase's decision) from a wrong implementation that used the
        // containing block's rect instead, which fixture()'s one-line
        // paragraphs (T1-T5, A1g) cannot: a single-line block's own top
        // coincides with any of its words' tops, so that mistake would pass
        // there by coincidence.
        await openHtmlDocWithContent(page, htmlAlignFixture());
        const iframe = page.frameLocator(HTML_IFRAME);
        await iframe.locator('p').first().waitFor({ timeout: 15_000 });

        const targetWord = iframe.locator('span').filter({ hasText: /^word40\.$/ }).first();
        await targetWord.scrollIntoViewIfNeeded();
        await page.waitForTimeout(300);

        const wordBox = await targetWord.boundingBox();
        expect(wordBox, 'precondition: the clicked word must be measurable').not.toBeNull();
        const hostY = wordBox!.y;

        const blockBox = await iframe
            .locator('p')
            .filter({ hasText: 'word1 word2' })
            .first()
            .boundingBox();
        expect(blockBox, 'precondition: the target paragraph must be measurable').not.toBeNull();
        expect(
            hostY - blockBox!.y,
            "precondition: word40 must render well below the paragraph's own top — i.e. the paragraph must actually wrap in the preview. If this fails, the fixture stopped wrapping (viewport width changed?) and this row can no longer tell a span-anchored hostY from a block-anchored one.",
        ).toBeGreaterThan(20);

        await targetWord.click();
        // handlePreviewSelection is not debounced, but Monaco's own smooth
        // scroll animation and the isSyncingRef suppression window both run
        // for up to 300ms (mirrors A1g's own wait).
        await page.waitForTimeout(600);

        // The row `revealEditorLine` actually positions is the target model
        // line's FIRST rendered row (`getTopForLineNumber` — alignment is
        // line-granular, not column-granular), regardless of which word in
        // that line was clicked or how the PREVIEW happened to wrap it. So
        // the row to measure in Monaco is the one starting the paragraph
        // (word1), not one containing word40 — word40 only supplies hostY.
        // `\s` (not a literal ASCII space) — Monaco renders spaces as
        // U+00A0 non-breaking space, which `\s` matches but a literal
        // `' '` in the regex would not (see editorVisibleText()'s doc
        // comment on the same gotcha).
        const firstRow = page
            .locator('.monaco-editor .view-lines .view-line')
            .filter({ hasText: /^word1\s/ })
            .first();
        await firstRow.waitFor({ timeout: 5000 });
        const lineBox = await firstRow.boundingBox();
        expect(lineBox, 'the target line must be rendered (in the DOM) after alignment').not.toBeNull();

        const ALIGNMENT_TOLERANCE_PX = 6;
        expect(
            Math.abs(lineBox!.y - hostY),
            `word40's on-screen y (${hostY}) and the target line's rendered y in Monaco (${lineBox!.y}) must agree within ${ALIGNMENT_TOLERANCE_PX}px`,
        ).toBeLessThanOrEqual(ALIGNMENT_TOLERANCE_PX);
    });
});
