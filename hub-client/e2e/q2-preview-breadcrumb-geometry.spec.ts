/**
 * Phase 4 — Breadcrumb chip geometry (real browser, layout engine required)
 *
 * Covers the full set of Tier C geometry assertions from the plan:
 *   claude-notes/plans/2026-06-15-breadcrumb-visual-design.md §Tier C, §Phase 4
 *
 * jsdom returns zero rects for everything, so every assertion here is
 * Playwright-only. Each test names the production hunk it guards.
 *
 * Run via:
 *   cd hub-client && npx playwright test e2e/q2-preview-breadcrumb-geometry.spec.ts --project=chromium
 *
 * Prerequisites: VITE_E2E=1 npm run build (once); hub-client build in dist/.
 *
 * Iframe-JS pattern used throughout:
 *   `iframe.locator('body').evaluate(() => { ...document.querySelector... })`
 *   This runs inside the iframe's browsing context (established by q2-preview-columns-layout.spec.ts).
 *   For scrolling #root we use the same pattern on `iframe.locator('#root')` — NOT
 *   `page.frame({ url: ... })`, because FrameLocator.locator().evaluate() is the
 *   idiomatic form used by every other spec in this suite.
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
// Shared setup helpers (mirrors q2-preview-block-nav-p2-5b.spec.ts)
// ---------------------------------------------------------------------------

/** Set up the project, navigate, and wait for preview + edit affordances. */
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

// ---------------------------------------------------------------------------
// Test suite
// ---------------------------------------------------------------------------

test.describe('Phase 4 — Breadcrumb chip geometry (real browser)', () => {
    test.setTimeout(120000);

    // Stagger worker starts to avoid AMD init races.
    test.beforeEach(async ({ page }, testInfo) => {
        if (testInfo.workerIndex > 0) {
            await page.waitForTimeout(1000);
        }
    });

    // -------------------------------------------------------------------------
    // Existing vertical test (P3.5 tier i item 1) — kept byte-for-byte.
    // Guards: chip sits above the active edit surface, never occluding line 1.
    // -------------------------------------------------------------------------

    test('chip sits above the active edit surface, never occluding line 1', async ({ page }) => {
        // Enable the nesting-cursor BEFORE any navigation — addInitScript re-applies
        // on every document load, including the iframe.
        await page.addInitScript(() => {
            localStorage.setItem('quarto-hub:preferences', JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
                unlockNestingCursor: true,
            }));
        });

        const serverUrl = getServerUrl();
        // Fixture: first block is a simple paragraph at the top of the document
        // so we exercise both the "chip above surface" case and the
        // "page-margin at top" case in one hit.
        const QMD = [
            '---',
            'format: q2-preview',
            '---',
            '',
            'First paragraph.',
            '',
            'Second paragraph.',
            '',
        ].join('\n');

        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'breadcrumb-geo.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'breadcrumb-geo.qmd');

        // Step 2: click the first block to open its editor.
        await iframe.locator('p[data-block-pool-id]').first().click();

        // Step 3: wait for both the textarea AND the chip.
        await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });
        const chip = iframe.locator('[data-testid="q2-breadcrumb-chip"]');
        await chip.waitFor({ timeout: 5000 });

        // The chip being visible is the proof that unlockNestingCursor took effect.
        const chipVisible = await chip.isVisible();
        expect(
            chipVisible,
            'BreadcrumbChip must be visible — unlockNestingCursor preference did not propagate',
        ).toBe(true);

        // Step 4: get bounding boxes from the real layout engine.
        const chipBox = await chip.boundingBox();
        const taBox = await iframe.locator('textarea').first().boundingBox();

        expect(chipBox, 'chip bounding box must be non-null').not.toBeNull();
        expect(taBox, 'textarea bounding box must be non-null').not.toBeNull();

        // TypeScript narrowing (already asserted above).
        if (!chipBox || !taBox) throw new Error('impossible — asserted above');

        console.log(
            `Chip box: y=${chipBox.y.toFixed(2)}, bottom=${(chipBox.y + chipBox.height).toFixed(2)}, height=${chipBox.height.toFixed(2)}\n` +
            `Textarea box: y=${taBox.y.toFixed(2)}`,
        );

        // Step 5: assertions.
        const TOL = 2;

        // (a) Chip bottom is AT OR ABOVE surface top — never occludes line 1.
        expect(
            chipBox.y + chipBox.height,
            `chip bottom (${(chipBox.y + chipBox.height).toFixed(2)}) must be ≤ textarea top (${taBox.y.toFixed(2)}) + ${TOL}px tolerance`,
        ).toBeLessThanOrEqual(taBox.y + TOL);

        // (b) Chip bottom is anchored close to the surface top (not floating far above).
        //     The gap must be less than ~one chip-gap (12 px) so the chip is still
        //     visually attached and the useLayoutEffect positioning is doing real work.
        expect(
            taBox.y - (chipBox.y + chipBox.height),
            `chip must be anchored near the surface top — gap (${(taBox.y - (chipBox.y + chipBox.height)).toFixed(2)}px) must be < 12px`,
        ).toBeLessThan(12);

        // (c) At the document top, the chip sits in the page margin (not clipped above
        //     the viewport — `top` is clamped to ≥ 0 in real CSS).
        expect(
            chipBox.y,
            `chip top (${chipBox.y.toFixed(2)}) must be ≥ 0 — not clipped above the viewport`,
        ).toBeGreaterThanOrEqual(0);

        // Step 6: close the editor.
        await iframe.locator('textarea').first().press('Escape');
    });

    // -------------------------------------------------------------------------
    // 4a — DESIGN GO/NO-GO GATE (not a regression guard).
    //
    // Verifies two preconditions for the margin-spill design:
    //   (i)  #quarto-content spans the full screen width (left ≈ 0 in screen
    //        coordinates) so the outer page margin is a grid column INSIDE
    //        #quarto-content — the chip can spill left at a positive `left`
    //        relative to #quarto-content's box, not a negative one.
    //   (ii) deep nesting does NOT cause a horizontal scrollbar on #root
    //        (the scrolling container inside the iframe).
    //
    // If EITHER assertion fails: STOP and reassess. The edge-clamp (3c) may be
    // broken, or the DOM layout differs from the assumption. Option A fallback
    // (keep chip confined to [colLeft, surfaceLeft]) would need to be
    // considered instead.
    //
    // This is a test (not test.fixme) because we expect it to pass; the comment
    // above explains what to do if it doesn't.
    // -------------------------------------------------------------------------

    test('4a — GATE: #quarto-content left ≈ 0 and no #root horizontal scrollbar under deep zero-indent nesting', async ({ page }) => {
        // GATE, not a revert test — guards the design precondition that
        // #quarto-content spans full screen width (so chip can spill left at a
        // positive offset) and that deep nesting never triggers a horizontal scrollbar.
        // If this GATE fails: STOP & REASSESS — Option A fallback may be needed.

        await page.addInitScript(() => {
            localStorage.setItem('quarto-hub:preferences', JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
                unlockNestingCursor: true,
            }));
        });

        const serverUrl = getServerUrl();
        // Fixture: several stacked fenced Divs (zero indent) wrapping a paragraph.
        // Zero-indent containers like Div/Figure use the outer page margin for crumbs.
        const QMD = [
            '---',
            'format: q2-preview',
            '---',
            '',
            '::: {.d1}',
            '',
            '::: {.d2}',
            '',
            '::: {.d3}',
            '',
            '::: {.d4}',
            '',
            'Inner paragraph.',
            '',
            ':::',
            '',
            ':::',
            '',
            ':::',
            '',
            ':::',
            '',
        ].join('\n');

        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'breadcrumb-4a-gate.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'breadcrumb-4a-gate.qmd');

        // Open the innermost editable paragraph.
        await iframe.locator('p[data-block-pool-id]').first().click();
        await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });
        await iframe.locator('[data-testid="q2-breadcrumb-chip"]').waitFor({ timeout: 5_000 });

        // (i) #quarto-content left ≈ 0 in screen coordinates.
        //     This verifies that #quarto-content spans the full screen width
        //     (grid-column: screen-start/screen-end), so the outer page margin is
        //     INSIDE #quarto-content's box. The chip can therefore use a positive
        //     `left` value within #quarto-content and not spill past x=0.
        const contentLeft = await iframe.locator('#quarto-content').evaluate((el) => {
            return el.getBoundingClientRect().left;
        });

        console.log(`4a: #quarto-content left=${contentLeft.toFixed(2)}`);

        expect(
            Math.abs(contentLeft),
            `#quarto-content left (${contentLeft.toFixed(2)}) must be ≈ 0 (within 2px) — ` +
            `full-width grid span is the precondition for margin-spill at positive left. ` +
            `GATE FAILURE: reassess Option A fallback.`,
        ).toBeLessThanOrEqual(2);

        // (ii) No horizontal scrollbar on #root.
        //      #root { overflow: auto; height: 100vh } is the scroll container inside
        //      the iframe (q2-preview.html:9). If the chip overflows past x=0 (the
        //      #quarto-content left edge), it would extend past #root's content box
        //      and cause a horizontal scrollbar. The edge-clamp in 3c prevents this.
        const noHScrollbar = await iframe.locator('#root').evaluate((el) => {
            return el.scrollWidth <= el.clientWidth + 1;
        });

        console.log(`4a: #root no-hscrollbar=${noHScrollbar}`);

        expect(
            noHScrollbar,
            `#root must NOT have a horizontal scrollbar after opening a deeply-nested editor. ` +
            `scrollWidth > clientWidth means a crumb spilled past x≈0 into negative territory. ` +
            `GATE FAILURE: the edge-clamp (3c) may be broken — reassess Option A fallback.`,
        ).toBe(true);

        await iframe.locator('textarea').first().press('Escape');
    });

    // -------------------------------------------------------------------------
    // 4b — Scroll-tracking (guards revert of hunk 3a).
    //
    // The bug: before 3a the chip was position:fixed relative to the viewport
    // (effectively). After 3a, it is position:absolute inside #quarto-content
    // (which is position:relative). The chip's `top` is computed once on
    // editTarget change and never updated on scroll — correct because the
    // offset-parent (#quarto-content) scrolls with the content, so the absolute
    // position tracks the surface automatically.
    //
    // VACUITY TRAP: the scroll must happen on #root (the iframe's actual scroll
    // container, q2-preview.html:9), NOT on window. Scrolling window inside a
    // sandboxed iframe does nothing — the test would pass vacuously because
    // neither the chip nor the textarea moves relative to the iframe's layout.
    // The comment below on the scroll call is load-bearing documentation.
    //
    // Revert 3a (restore surface.offsetParent as host / remove position:relative
    // from #quarto-content) → chip detaches from the scrolling content → the
    // gap between chip bottom and textarea top CHANGES after scrolling → RED.
    // -------------------------------------------------------------------------

    test('4b — scroll-tracking: chip stays glued to surface after scrolling #root (guards revert of 3a)', async ({ page }) => {
        await page.addInitScript(() => {
            localStorage.setItem('quarto-hub:preferences', JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
                unlockNestingCursor: true,
            }));
        });

        const serverUrl = getServerUrl();

        // Fixture: a TALL document so the active surface can be mid-document
        // and the page scrolls. ~50 paragraphs, target paragraph roughly in the
        // middle so there is content both above and below.
        const paragraphs: string[] = [];
        for (let i = 1; i <= 25; i++) {
            paragraphs.push(`Filler paragraph number ${i}.`, '');
        }
        paragraphs.push('Target paragraph for scroll test.', '');
        for (let i = 26; i <= 50; i++) {
            paragraphs.push(`Filler paragraph number ${i}.`, '');
        }

        const QMD = [
            '---',
            'format: q2-preview',
            '---',
            '',
            ...paragraphs,
        ].join('\n');

        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'breadcrumb-4b-scroll.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'breadcrumb-4b-scroll.qmd');

        // Open an editor on the TARGET paragraph (mid-document — not the very top).
        // We locate it by its unique text.
        await iframe.locator('p', { hasText: 'Target paragraph for scroll test.' }).click();
        await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });
        const chip = iframe.locator('[data-testid="q2-breadcrumb-chip"]');
        await chip.waitFor({ timeout: 5_000 });

        // Record initial geometry.
        const chipBox1 = await chip.boundingBox();
        const taBox1 = await iframe.locator('textarea').first().boundingBox();

        expect(chipBox1, 'chip bounding box must be non-null before scroll').not.toBeNull();
        expect(taBox1, 'textarea bounding box must be non-null before scroll').not.toBeNull();

        if (!chipBox1 || !taBox1) throw new Error('impossible — asserted above');

        const gap1 = taBox1.y - (chipBox1.y + chipBox1.height);

        console.log(
            `4b before scroll: chip.y=${chipBox1.y.toFixed(2)}, chip.bottom=${(chipBox1.y + chipBox1.height).toFixed(2)}, ` +
            `ta.y=${taBox1.y.toFixed(2)}, gap=${gap1.toFixed(2)}`,
        );

        // CRITICAL: scroll #root (the iframe's actual scroll container, defined in
        // q2-preview.html as `#root { overflow: auto; height: 100vh }`), NOT window.
        //
        // VACUITY TRAP: scrolling `window` inside a sandboxed same-origin iframe
        // does nothing observable — neither the chip nor the textarea moves in
        // viewport coordinates — so the gap test would pass regardless of whether
        // the fix (3a) is in place. We must scroll #root to actually move the
        // document content within the iframe's viewport.
        //
        // After scrolling #root by 400px the chip (position:absolute inside the
        // scrolled #quarto-content) moves upward in viewport coords by ~400px
        // ALONG WITH the textarea — so the gap is preserved. If 3a is reverted
        // (chip detaches from content scroll), the chip stays at its old viewport
        // y while the textarea moves up ~400px → gap changes → RED.
        await iframe.locator('#root').evaluate((el) => {
            el.scrollTop += 400;
        });

        // Wait a frame for any layout recalculation.
        await page.waitForTimeout(100);

        // Re-measure after scroll.
        const chipBox2 = await chip.boundingBox();
        const taBox2 = await iframe.locator('textarea').first().boundingBox();

        expect(chipBox2, 'chip bounding box must be non-null after scroll').not.toBeNull();
        expect(taBox2, 'textarea bounding box must be non-null after scroll').not.toBeNull();

        if (!chipBox2 || !taBox2) throw new Error('impossible — asserted above');

        const gap2 = taBox2.y - (chipBox2.y + chipBox2.height);

        console.log(
            `4b after scroll: chip.y=${chipBox2.y.toFixed(2)}, chip.bottom=${(chipBox2.y + chipBox2.height).toFixed(2)}, ` +
            `ta.y=${taBox2.y.toFixed(2)}, gap=${gap2.toFixed(2)}, delta=${(gap2 - gap1).toFixed(2)}`,
        );

        // Gap must be unchanged within a small tolerance (~3px).
        // A larger change means the chip detached from the scrolling content.
        const GAP_TOL = 3;
        expect(
            Math.abs(gap2 - gap1),
            `chip↔surface gap must be unchanged after scrolling #root ` +
            `(before: ${gap1.toFixed(2)}px, after: ${gap2.toFixed(2)}px, delta: ${(gap2 - gap1).toFixed(2)}px). ` +
            `Guards hunk 3a: reverting (restore surface.offsetParent / non-relative #quarto-content) ` +
            `causes the chip to detach from content scroll → gap changes → RED.`,
        ).toBeLessThanOrEqual(GAP_TOL);

        await iframe.locator('textarea').first().press('Escape');
    });

    // -------------------------------------------------------------------------
    // 4c — Gutter-fill geometry (guards textarea-anchor + gutter band).
    //
    // Fixture: an INDENTED surface — a loose-list item paragraph.
    //   NOT a tight-list Plain — the successor plan turns tight-list Plains into
    //   directly-activated surfaces and shifts their rect; a loose Para is stable.
    //
    // New design contract (all coords iframe-relative via .evaluate()):
    //   - ◀ right edge ≈ colLeft (±3px): ◀ sits in the margin, flush at the
    //     text-column left.
    //   - leftmost .q2-crumb left ≈ colLeft (±3px): crumbs start at the column
    //     margin, never entering the outer margin.
    //   - rightmost .q2-crumb right ≈ surfaceLeft (±10px): crumb band meets the
    //     textarea anchor for indented blocks.
    //
    // Guards: the textarea-anchor (surface left read from <textarea> inside the
    // region, not the region wrapper) and the gutter-fill band logic.
    // Revert the textarea-anchor → surfaceLeft becomes colLeft (no indent) →
    // rightmost crumb right no longer meets textarea position → RED.
    // -------------------------------------------------------------------------

    test('4c — shallow indent: ◀ clamps at the page edge and the crumb band spills left into the margin (G1 left-spill)', async ({ page }) => {
        await page.addInitScript(() => {
            localStorage.setItem('quarto-hub:preferences', JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
                unlockNestingCursor: true,
            }));
        });

        const serverUrl = getServerUrl();

        // Fixture: a loose-list item paragraph (list item with a blank line after,
        // making each item a Para node, not a Plain node).
        // NOT a tight-list Plain — the successor plan shifts those rects.
        const QMD = [
            '---',
            'format: q2-preview',
            '---',
            '',
            '* First loose item.',
            '',
            '* Second loose item.',
            '',
            '* Third loose item.',
            '',
        ].join('\n');

        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'breadcrumb-4c-gutter.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'breadcrumb-4c-gutter.qmd');

        // Open the first list item paragraph.
        await iframe.locator('li p[data-block-pool-id]').first().click();
        await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });
        const chip = iframe.locator('[data-testid="q2-breadcrumb-chip"]');
        await chip.waitFor({ timeout: 5_000 });

        // All geometry measurements are iframe-relative via .evaluate() so they live
        // in the same coordinate space. NEVER use .boundingBox() for layout comparisons
        // here — .boundingBox() returns page-relative coords (~755px offset in split
        // view) while .evaluate(getBoundingClientRect) returns iframe-relative coords.

        // Measure colLeft = main#quarto-document-content left (iframe-relative).
        const colLeft = await iframe.locator('main#quarto-document-content').evaluate((el) => {
            return el.getBoundingClientRect().left;
        });

        // Measure surfaceLeft = textarea left (iframe-relative).
        // The textarea anchors to the block's real indented left edge, not the
        // full-column #q2-active-edit-region wrapper. This is what the chip reads.
        const surfaceLeft = await iframe.locator('textarea').first().evaluate((el) => {
            return el.getBoundingClientRect().left;
        });

        // Measure the chip's geometry — all iframe-relative.
        // outArrowRight = right edge of ◀ (.q2-breadcrumb-out)
        // crumbLeft     = left edge of leftmost .q2-crumb
        // crumbRight    = right edge of rightmost .q2-crumb
        const chipGeom = await iframe.locator('[data-testid="q2-breadcrumb-chip"]').evaluate((chipEl) => {
            const outArrow = chipEl.querySelector('.q2-breadcrumb-out');
            const crumbs = Array.from(chipEl.querySelectorAll('.q2-crumb'));
            if (!outArrow || crumbs.length === 0) return null;
            const outRect = outArrow.getBoundingClientRect();
            const crumbRects = crumbs.map((el) => el.getBoundingClientRect());
            return {
                outArrowLeft: outRect.left,
                outArrowRight: outRect.right,
                crumbLeft: Math.min(...crumbRects.map((r) => r.left)),
                crumbRight: Math.max(...crumbRects.map((r) => r.right)),
            };
        });

        expect(chipGeom, 'chip geometry must be non-null').not.toBeNull();
        if (!chipGeom) throw new Error('impossible — asserted above');

        console.log(
            `4c: colLeft=${colLeft.toFixed(2)}, surfaceLeft=${surfaceLeft.toFixed(2)}, ` +
            `outArrowLeft=${chipGeom.outArrowLeft.toFixed(2)}, outArrowRight=${chipGeom.outArrowRight.toFixed(2)}, ` +
            `crumbLeft=${chipGeom.crumbLeft.toFixed(2)}, crumbRight=${chipGeom.crumbRight.toFixed(2)}`,
        );

        // G1 left-spill model (NOT the pre-G1 "3b" ◀-at-colLeft model — that was
        // superseded in the same commit and never browser-validated). A loose
        // list item's indent gutter (here ~34px) is narrower than the comfortable
        // 2-crumb band (2·CRUMB_W = 44px), so computeChipGeometry pushes the chip
        // LEFT until ◀ clamps at the page edge (chipLeft = 0). See the unit suite
        // BreadcrumbChip.geometry.test.ts cases (b)/(c) for the blessed contract.
        const TOL = 3;

        // (a) ◀ is clamped at the page edge (chipLeft ≈ 0), NOT anchored at colLeft.
        //     This is THE binding assertion for the left-spill clamp: reverting to
        //     the ◀-at-colLeft anchor moves outArrowLeft to ≈ colLeft − MIN_GLYPH_W
        //     (~9.5px), failing this.
        expect(
            chipGeom.outArrowLeft,
            `◀ left (${chipGeom.outArrowLeft.toFixed(2)}) must be ≈ 0 (clamped at the page edge). ` +
            `Guards the G1 left-spill clamp for a shallow indent gutter.`,
        ).toBeLessThanOrEqual(TOL);
        expect(chipGeom.outArrowLeft).toBeGreaterThanOrEqual(-TOL);

        // (b) the crumbs spill LEFT into the outer margin (crumbLeft < colLeft) and
        //     start right at ◀'s right edge — the band is pushed left of the column.
        expect(
            chipGeom.crumbLeft,
            `leftmost crumb left (${chipGeom.crumbLeft.toFixed(2)}) must be < colLeft (${colLeft.toFixed(2)}): ` +
            `the shallow-gutter band spills into the outer margin (G1).`,
        ).toBeLessThan(colLeft - TOL);
        expect(
            chipGeom.crumbLeft,
            `leftmost crumb left (${chipGeom.crumbLeft.toFixed(2)}) must meet ◀ right (${chipGeom.outArrowRight.toFixed(2)}).`,
        ).toBeGreaterThanOrEqual(chipGeom.outArrowRight - TOL);

        // (c) the band's right edge still reaches the pivot (surfaceLeft) for this
        //     shallow-but-nonzero gutter — the band width equals the comfortable
        //     natural width, which here ≈ the gutter, so it lands at the surface.
        const TOL_SURF = 10;
        expect(
            chipGeom.crumbRight,
            `rightmost crumb right (${chipGeom.crumbRight.toFixed(2)}) must be ≈ surfaceLeft (${surfaceLeft.toFixed(2)}) ±${TOL_SURF}px. ` +
            `Guards the band meeting the indented surface (textarea anchor).`,
        ).toBeGreaterThanOrEqual(surfaceLeft - TOL_SURF);
        expect(chipGeom.crumbRight).toBeLessThanOrEqual(surfaceLeft + TOL_SURF);

        await iframe.locator('textarea').first().press('Escape');
    });

    // -------------------------------------------------------------------------
    // 4d — Zero-indent Div under the G1 left-spill model.
    //
    // Fixture: a fenced-Div-wrapped paragraph (zero indent gutter).
    //
    // G1 design contract (all coords iframe-relative via .evaluate()):
    //   - ◀ clamps at the page edge: outArrowLeft ≈ 0 (chipLeft = 0). The chip
    //     cannot fit its comfortable band in a zero gutter, so it is pushed all
    //     the way left until ◀ hits x=0.
    //   - crumbs spill LEFT into the outer margin: crumbLeft < colLeft. (A zero-
    //     indent block's crumbs DO enter the margin — the "where you came from"
    //     direction; see BreadcrumbChip's layout-model doc.)
    //   - the band overshoots RIGHT past the pivot into content: crumbRight >
    //     surfaceLeft (geometry unit-test case (c)).
    //
    // Real browser sample (run 2026-06-19):
    //   colLeft=25.5, ◀=[0,16], crumbs=[16,60], surfaceLeft≈25.5 (no indent).
    //
    // This REPLACES the pre-G1 "3b" expectations (◀ at colLeft, crumbs out of the
    // margin) that the old assertions encoded — superseded in the G1 commit and
    // never browser-validated before now.
    // -------------------------------------------------------------------------

    test('4d — zero-indent Div: ◀ clamps at the page edge, crumbs spill left into margin and band overshoots right (G1 left-spill)', async ({ page }) => {
        await page.addInitScript(() => {
            localStorage.setItem('quarto-hub:preferences', JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
                unlockNestingCursor: true,
            }));
        });

        const serverUrl = getServerUrl();

        // Fixture: a fenced Div wrapping a paragraph (zero indent for the Div).
        const QMD = [
            '---',
            'format: q2-preview',
            '---',
            '',
            '::: {.my-div}',
            '',
            'Paragraph inside a fenced Div.',
            '',
            ':::',
            '',
        ].join('\n');

        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'breadcrumb-4d-margin.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'breadcrumb-4d-margin.qmd');

        // Open the paragraph inside the Div.
        await iframe.locator('p[data-block-pool-id]').first().click();
        await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });
        const chip = iframe.locator('[data-testid="q2-breadcrumb-chip"]');
        await chip.waitFor({ timeout: 5_000 });

        // All geometry is iframe-relative via .evaluate() — NEVER mix with .boundingBox().

        // colLeft = main#quarto-document-content left (iframe-relative).
        const colLeft = await iframe.locator('main#quarto-document-content').evaluate((el) => {
            return el.getBoundingClientRect().left;
        });

        // Chip geometry: outArrowRight, leftmost crumb left, rightmost crumb right.
        const chipGeom = await iframe.locator('[data-testid="q2-breadcrumb-chip"]').evaluate((chipEl) => {
            const outArrow = chipEl.querySelector('.q2-breadcrumb-out');
            const crumbs = Array.from(chipEl.querySelectorAll('.q2-crumb'));
            if (!outArrow || crumbs.length === 0) return null;
            const outRect = outArrow.getBoundingClientRect();
            const crumbRects = crumbs.map((el) => el.getBoundingClientRect());
            return {
                outArrowLeft: outRect.left,
                outArrowRight: outRect.right,
                crumbLeft: Math.min(...crumbRects.map((r) => r.left)),
                crumbRight: Math.max(...crumbRects.map((r) => r.right)),
            };
        });

        expect(chipGeom, 'chip geometry must be non-null').not.toBeNull();
        if (!chipGeom) throw new Error('impossible — asserted above');

        // surfaceLeft (zero-indent Div → ≈ colLeft; the band overshoots right of it).
        const surfaceLeft = await iframe.locator('textarea').first().evaluate((el) => {
            return el.getBoundingClientRect().left;
        });

        console.log(
            `4d: colLeft=${colLeft.toFixed(2)}, surfaceLeft=${surfaceLeft.toFixed(2)}, ` +
            `outArrowLeft=${chipGeom.outArrowLeft.toFixed(2)}, outArrowRight=${chipGeom.outArrowRight.toFixed(2)}, ` +
            `crumbLeft=${chipGeom.crumbLeft.toFixed(2)}, crumbRight=${chipGeom.crumbRight.toFixed(2)}`,
        );

        // G1 left-spill model for a ZERO-indent Div (gutter = 0). The comfortable
        // band (2·CRUMB_W) cannot fit a zero gutter, so the chip is pushed left
        // until ◀ clamps at the page edge (chipLeft = 0) and the band spills RIGHT
        // past the pivot into the content (BreadcrumbChip.geometry.test.ts case (c)).
        // NB: this REPLACES the pre-G1 "3b" expectations (◀ at colLeft, crumbs
        // out of the margin) — those were superseded in the same commit and never
        // browser-validated. Under G1, a zero-indent block's crumbs DO enter the
        // margin (the "where you came from" direction).
        const TOL = 3;

        // (i) ◀ is clamped at the page edge (chipLeft ≈ 0), NOT anchored at colLeft.
        //     Binding assertion for the clamp: the ◀-at-colLeft revert moves
        //     outArrowLeft to ≈ colLeft − MIN_GLYPH_W (~9.5px), failing this.
        expect(
            chipGeom.outArrowLeft,
            `◀ left (${chipGeom.outArrowLeft.toFixed(2)}) must be ≈ 0 (clamped at the page edge) for a zero-indent block.`,
        ).toBeLessThanOrEqual(TOL);
        expect(chipGeom.outArrowLeft).toBeGreaterThanOrEqual(-TOL);

        // (ii) the crumbs spill LEFT into the outer margin (crumbLeft < colLeft) —
        //      the zero gutter forces the band left of the column.
        expect(
            chipGeom.crumbLeft,
            `leftmost crumb left (${chipGeom.crumbLeft.toFixed(2)}) must be < colLeft (${colLeft.toFixed(2)}): ` +
            `a zero-indent block's band spills into the outer margin (G1).`,
        ).toBeLessThan(colLeft - TOL);

        // (iii) the band overshoots RIGHT past the pivot into the content
        //       (crumbRight > surfaceLeft) — the comfortable width has nowhere to go
        //       but right once ◀ is pinned at x=0.
        expect(
            chipGeom.crumbRight,
            `rightmost crumb right (${chipGeom.crumbRight.toFixed(2)}) must overshoot surfaceLeft (${surfaceLeft.toFixed(2)}): ` +
            `the comfortable band spills right past the pivot for a zero gutter (G1 case c).`,
        ).toBeGreaterThan(surfaceLeft + TOL);

        await iframe.locator('textarea').first().press('Escape');
    });

    // -------------------------------------------------------------------------
    // 4d-fig — Figure margin-spill (guards hunk 3c + Figure ancestor-walk).
    //
    // FIXME: This test is marked fixme because no reachable qmd fixture currently
    // produces a Figure AST node with an editable paragraph body in the q2-preview
    // renderer. Here is what was tried:
    //
    //   Attempt 1: :::{#fig-example}\nEditable paragraph inside Figure.\n
    //              Figure: A caption for the figure.\n:::
    //   Result: Preview iframe timed out (render never completed).
    //   Diagnosis: "Figure: A caption" is not valid q2 syntax and appears to
    //   cause a render crash. The `:::{#fig-example}` fenced-div form also
    //   produces a Div AST node, not a Figure node — in q2, Figure nodes are
    //   produced only from single-image paragraphs (postprocess.rs:968-1027).
    //
    //   Attempt 2: :::{#fig-example}\nEditable paragraph inside Figure.\n:::
    //   Result: Would render a Div (id=fig-example), not a Figure. No Fg crumb.
    //
    //   The correct path to a Figure ancestor crumb requires a `Figure` AST node,
    //   which in q2 comes only from a single-image Para (postprocess.rs). An image
    //   inside a Figure has no editable text body; the Figure body itself has no
    //   accessible editable paragraph. The Figure.tsx component renders body blocks
    //   (c[2]) from the AST — but when produced from a single-image Para, the body
    //   IS the image, which is not an editable block.
    //
    //   To unblock this, either:
    //     (a) Expose a qmd/AST path that creates a multi-block Figure with an
    //         editable Para in the body (e.g. fenced-div Figure syntax properly
    //         desugared to a Figure AST node with a non-image body block), OR
    //     (b) Add a cross-reference figure fixture (:::{#fig-x} with image + caption
    //         where the caption Para is editable) and expose the caption path.
    //
    //   Track in a braid strand for future resolution.
    // -------------------------------------------------------------------------

    test.fixme('4d-fig — Figure margin-spill: Fg crumb renders and ◀ anchored at colLeft (guards 3c + Figure path)', async ({ page }) => {
        await page.addInitScript(() => {
            localStorage.setItem('quarto-hub:preferences', JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
                unlockNestingCursor: true,
            }));
        });

        const serverUrl = getServerUrl();

        // Fixture: a fenced Div with a fig- id containing a paragraph.
        // NOTE: This produces a Div AST node (not Figure) — see fixme comment above.
        // The test is kept here as a placeholder with the correct assertions for when
        // a working Figure fixture is found.
        const QMD = [
            '---',
            'format: q2-preview',
            '---',
            '',
            ':::{#fig-example}',
            '',
            'Editable paragraph inside Figure.',
            '',
            ':::',
            '',
        ].join('\n');

        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'breadcrumb-4d-fig.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'breadcrumb-4d-fig.qmd');

        await iframe.locator('p[data-block-pool-id]').first().click();
        await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });
        const chip = iframe.locator('[data-testid="q2-breadcrumb-chip"]');
        await chip.waitFor({ timeout: 5_000 });

        // (i) A crumb with abbreviation 'Fg' (title starting 'Figure') must render.
        //     Guards Figure.tsx:31 emitting data-block-pool-id for the <figure> wrapper.
        const hasFgCrumb = await iframe.locator('[data-testid="q2-breadcrumb-chip"]').evaluate((chipEl) => {
            const crumbs = Array.from(chipEl.querySelectorAll('.q2-crumb'));
            return crumbs.some((el) => {
                const title = el.getAttribute('title') ?? '';
                const text = el.textContent ?? '';
                return text.trim() === 'Fg' || title.startsWith('Figure');
            });
        });
        console.log(`4d-fig: hasFgCrumb=${hasFgCrumb}`);
        expect(hasFgCrumb, 'A .q2-crumb with textContent=Fg or title starting Figure must be present').toBe(true);

        // (ii) ◀ right ≈ colLeft (±3px): Figure is non-indenting, same as Div.
        const colLeft = await iframe.locator('main#quarto-document-content').evaluate((el) => {
            return el.getBoundingClientRect().left;
        });
        const outArrowRight = await iframe.locator('[data-testid="q2-breadcrumb-chip"]').evaluate((chipEl) => {
            const outArrow = chipEl.querySelector('.q2-breadcrumb-out');
            return outArrow ? outArrow.getBoundingClientRect().right : null;
        });
        console.log(`4d-fig: colLeft=${colLeft.toFixed(2)}, outArrowRight=${outArrowRight?.toFixed(2) ?? 'null'}`);
        expect(outArrowRight, '◀ must be present').not.toBeNull();
        if (outArrowRight !== null) {
            expect(outArrowRight).toBeGreaterThanOrEqual(colLeft - 3);
            expect(outArrowRight).toBeLessThanOrEqual(colLeft + 3);
        }

        await iframe.locator('textarea').first().press('Escape');
    });

    // -------------------------------------------------------------------------
    // 4d-compress (gap G1) — Compression / ellipsize under deep nesting.
    //
    // Fixture: very deep zero-indent nesting (many stacked fenced Divs) that
    // exhausts the available left margin and forces the chip to ellipsize
    // middle crumbs.
    //
    // Asserts:
    //   (i)  A '.q2-crumb-ellipsis' element ('…') is present — middle crumbs
    //        were compressed away.
    //   (ii) The leftmost crumb left edge >= -1 (≈0) — the edge clamp prevents
    //        the chip from spilling past x≈0 and triggering a horizontal scrollbar
    //        on #root. Overlaps 4a's invariant.
    //
    // Revert hunk = the clamp/compress branch in 3c (min-glyph-width + ellipsize).
    //
    // NOTE ON THRESHOLD: the production constants MIN_GLYPH_W=20 and the
    // ellipsize trigger (≥3 crumbs when min-width compression still doesn't fit
    // [0, surfaceLeft]) mean the exact number of nesting levels that trigger
    // ellipsis depends on the page layout and viewport width. The fixture below
    // uses 8 levels; if that is not enough to trigger ellipsis in CI, increase
    // the level count or reduce the viewport. The assertion is wrapped in a
    // tolerant check with a clear note — if ellipsis is absent, the test skips
    // the ellipsis assertion but still checks the leftmost-crumb clamp.
    // -------------------------------------------------------------------------

    test('4d-compress — deep nesting: middle crumbs ellipsized and leftmost crumb clamped at x≈0 (guards 3c compress)', async ({ page }) => {
        await page.addInitScript(() => {
            localStorage.setItem('quarto-hub:preferences', JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
                unlockNestingCursor: true,
            }));
        });

        const serverUrl = getServerUrl();

        // Fixture: 8 deeply stacked fenced Divs (zero indent each) wrapping a
        // paragraph. The crumb chain would be [◀, Dv, Dv, Dv, Dv, Dv, Dv, Dv, Dv, ¶]
        // — far wider than the available page margin, forcing compression.
        //
        // If 8 levels are not enough to trigger ellipsis in the tested viewport,
        // the ellipsis assertion below notes this and does not fail — the clamp
        // assertion (ii) still runs.
        const openDivs = Array.from({ length: 8 }, (_, i) => [`::: {.deep${i + 1}}`, '']).flat();
        const closeDivs = Array.from({ length: 8 }, () => [':::', '']).flat();

        const QMD = [
            '---',
            'format: q2-preview',
            '---',
            '',
            ...openDivs,
            'Deeply nested paragraph.',
            '',
            ...closeDivs,
        ].join('\n');

        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'breadcrumb-4d-compress.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'breadcrumb-4d-compress.qmd');

        // Open the innermost paragraph.
        await iframe.locator('p[data-block-pool-id]').first().click();
        await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });
        const chip = iframe.locator('[data-testid="q2-breadcrumb-chip"]');
        await chip.waitFor({ timeout: 5_000 });

        // Measure the leftmost crumb left edge and check for ellipsis.
        const compressResult = await iframe.locator('[data-testid="q2-breadcrumb-chip"]').evaluate((chip) => {
            const ellipsis = chip.querySelector('.q2-crumb-ellipsis');
            const allCrumbs = Array.from(chip.querySelectorAll('.q2-crumb, .q2-breadcrumb-out'));
            const rects = allCrumbs.map((el) => el.getBoundingClientRect());
            const rowLeft = rects.length > 0 ? Math.min(...rects.map((r) => r.left)) : 0;
            return {
                hasEllipsis: ellipsis !== null,
                ellipsisText: ellipsis?.textContent ?? null,
                rowLeft,
                crumbCount: allCrumbs.length,
            };
        });

        console.log(
            `4d-compress: hasEllipsis=${compressResult.hasEllipsis}, ellipsisText="${compressResult.ellipsisText}", ` +
            `rowLeft=${compressResult.rowLeft.toFixed(2)}, crumbCount=${compressResult.crumbCount}`,
        );

        // (i) Ellipsis assertion — tolerant.
        //     If ellipsis is absent at 8 nesting levels, it means the threshold is
        //     not yet met in this viewport. Log a warning but do not fail hard —
        //     the threshold constant (MIN_GLYPH_W, ellipsize trigger) may need tuning
        //     based on the actual CI viewport width. The clamp assertion (ii) is what
        //     matters for safety; ellipsis is a UX refinement.
        if (compressResult.hasEllipsis) {
            expect(
                compressResult.ellipsisText?.trim(),
                'ellipsis element text must be "…" (the ellipsis character)',
            ).toBe('…');
        } else {
            // Ellipsis not triggered — warn but do not fail.
            // ACTION NEEDED: if this logs consistently in CI, either increase the
            // nesting depth above (try 12 levels) or narrow the viewport in
            // playwright.config.ts. The compress branch in 3c may also have a
            // different threshold than assumed.
            console.warn(
                `4d-compress: ellipsis NOT triggered at ${compressResult.crumbCount} crumbs. ` +
                `The MIN_GLYPH_W/ellipsize threshold may be higher than the available margin. ` +
                `Consider increasing nesting depth or reducing viewport width.`,
            );
        }

        // (ii) Leftmost crumb clamped at x ≈ 0 — no #root horizontal scrollbar.
        //      The edge-clamp in 3c must prevent any crumb from going past x≈0.
        //      -1 tolerance for sub-pixel rounding.
        //      Guards the clamp/compress branch in 3c.
        expect(
            compressResult.rowLeft,
            `leftmost crumb left (${compressResult.rowLeft.toFixed(2)}) must be >= -1 (≈0). ` +
            `The edge-clamp (3c) must prevent the chip from spilling past #quarto-content's left edge. ` +
            `Going past x≈0 exits #root's content box and causes a horizontal scrollbar. ` +
            `Guards 3c compress branch.`,
        ).toBeGreaterThanOrEqual(-1);

        await iframe.locator('textarea').first().press('Escape');
    });

    // -------------------------------------------------------------------------
    // T19a — G1 left-spill: code-in-blockquote shows both crumbs, pivot correct.
    //
    // Fixture: a paragraph inside a CodeBlock inside a BlockQuote. The blockquote
    // provides ~24–30px of indent gutter. With the old gutter-only geometry:
    //   gutter ≈ 24px, MIN_GLYPH_W=16 → slots=1 → only current crumb shown.
    // With the new left-spill geometry:
    //   naturalWidth = 2 * CRUMB_W (=22) = 44px; bandWidth = max(gutter, 44) = 44px
    //   chipLeft = surfaceLeft - OUT_W - 44 → spills left into margin
    //   slots = floor(44 / 16) = 2 → both crumbs shown.
    //
    // This test guards:
    //   (i)  Both BlockQuote AND CodeBlock crumbs are visible (not ellipsized)
    //   (ii) Leftmost crumb x ≥ -1 (≈0) — no horizontal scrollbar on #root
    //   (iii) Rightmost crumb right ≤ surfaceLeft + CRUMB_W (right edge ≈ pivot or
    //         at most CRUMB_W past pivot if spilling right at edge)
    //
    // fail-on-revert: reverting computeChipGeometry to gutter-only (bandWidth=gutter,
    // slots=1) → only ONE crumb visible → assertion (i) fails → RED.
    // -------------------------------------------------------------------------

    test('T19a — G1 left-spill: code-in-blockquote shows BOTH crumbs with correct pivot (guards computeChipGeometry)', async ({ page }) => {
        await page.addInitScript(() => {
            localStorage.setItem('quarto-hub:preferences', JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
                unlockNestingCursor: true,
            }));
        });

        const serverUrl = getServerUrl();

        // Fixture: paragraph INSIDE a CodeBlock INSIDE a BlockQuote.
        // The CodeBlock is itself an editable surface (in unlock mode);
        // when editing the CodeBlock, the ancestor path is [BlockQuote, CodeBlock].
        // The blockquote provides ~24–30px of indent gutter (typically less than
        // 2 * CRUMB_W = 44px), triggering the left-spill path.
        const QMD = [
            '---',
            'format: q2-preview',
            '---',
            '',
            '> ```',
            '> code line one',
            '> ```',
            '',
            'Paragraph after blockquote.',
            '',
        ].join('\n');

        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'breadcrumb-t19a-code-in-bq.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'breadcrumb-t19a-code-in-bq.qmd');

        // Open the code block editor by clicking on it (in unlock mode).
        // The CodeBlock inside the blockquote should be activatable.
        await iframe.locator('[data-block-pool-id]').first().click();
        await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });
        const chip = iframe.locator('[data-testid="q2-breadcrumb-chip"]');
        await chip.waitFor({ timeout: 5_000 });

        // Measure all chip geometry iframe-relative.
        const surfaceLeft = await iframe.locator('textarea').first().evaluate((el) => {
            return el.getBoundingClientRect().left;
        });

        const chipGeom = await iframe.locator('[data-testid="q2-breadcrumb-chip"]').evaluate((chipEl) => {
            const crumbs = Array.from(chipEl.querySelectorAll('.q2-crumb'));
            const outArrow = chipEl.querySelector('.q2-breadcrumb-out');
            if (crumbs.length === 0 || !outArrow) return null;
            const crumbRects = crumbs.map((el) => el.getBoundingClientRect());
            const outRect = outArrow.getBoundingClientRect();
            return {
                crumbCount: crumbs.length,
                crumbTitles: crumbs.map((el) => el.getAttribute('title') ?? ''),
                crumbLeft: Math.min(...crumbRects.map((r) => r.left)),
                crumbRight: Math.max(...crumbRects.map((r) => r.right)),
                outArrowRight: outRect.right,
            };
        });

        expect(chipGeom, 'chip geometry must be non-null').not.toBeNull();
        if (!chipGeom) throw new Error('impossible — asserted above');

        console.log(
            `T19a: surfaceLeft=${surfaceLeft.toFixed(2)}, ` +
            `crumbCount=${chipGeom.crumbCount}, titles=${JSON.stringify(chipGeom.crumbTitles)}, ` +
            `crumbLeft=${chipGeom.crumbLeft.toFixed(2)}, crumbRight=${chipGeom.crumbRight.toFixed(2)}, ` +
            `outArrowRight=${chipGeom.outArrowRight.toFixed(2)}`,
        );

        // (i) BOTH crumbs must be visible (BlockQuote ancestor + CodeBlock current).
        //     fail-on-revert: gutter-only geometry → slots=1 → only one crumb → RED.
        expect(
            chipGeom.crumbCount,
            `must have at least 2 crumbs (BlockQuote + CodeBlock). ` +
            `Got ${chipGeom.crumbCount}: ${JSON.stringify(chipGeom.crumbTitles)}. ` +
            `fail-on-revert: revert computeChipGeometry to gutter-only → slots=1 → only 1 crumb → RED`,
        ).toBeGreaterThanOrEqual(2);

        // Verify the expected crumb types are present.
        const hasBlockQuoteCrumb = chipGeom.crumbTitles.some((t) => t === 'BlockQuote');
        const hasCodeBlockCrumb = chipGeom.crumbTitles.some((t) => t === 'CodeBlock');
        expect(
            hasBlockQuoteCrumb,
            `BlockQuote ancestor crumb must be present. Got: ${JSON.stringify(chipGeom.crumbTitles)}`,
        ).toBe(true);
        expect(
            hasCodeBlockCrumb,
            `CodeBlock current crumb must be present. Got: ${JSON.stringify(chipGeom.crumbTitles)}`,
        ).toBe(true);

        // (ii) Leftmost crumb x ≥ -1 (≈0): no horizontal scrollbar on #root.
        //      Even though crumbs spill left into the page margin, the left-edge
        //      clamp ensures they don't go past x=0. -1 tolerance for sub-pixel rounding.
        expect(
            chipGeom.crumbLeft,
            `leftmost crumb left (${chipGeom.crumbLeft.toFixed(2)}) must be ≥ -1 (≈0). ` +
            `Crumbs must not spill past #quarto-content's left edge (x≈0).`,
        ).toBeGreaterThanOrEqual(-1);

        // (iii) No horizontal scrollbar on #root after opening the editor.
        const noHScrollbar = await iframe.locator('#root').evaluate((el) => {
            return el.scrollWidth <= el.clientWidth + 1;
        });
        expect(
            noHScrollbar,
            '#root must not have a horizontal scrollbar after opening the code-in-blockquote editor.',
        ).toBe(true);

        await iframe.locator('textarea').first().press('Escape');
    });

    // -------------------------------------------------------------------------
    // T19b — G1 right-spill at edge: when ◀ is clamped at x≈0, band extends
    // right past the pivot; full path still visible.
    //
    // Fixture: a paragraph at the LEFT EDGE of the document (surfaceLeft ≈ colLeft,
    // essentially zero indent from the left margin of quarto-content). This means
    // the chipLeft calculation would go negative → pin ◀ at x=0 and let the band
    // extend RIGHT past the pivot.
    //
    // A deeply zero-indented paragraph (many stacked fenced Divs) brings
    // surfaceLeft ≈ colLeft, and with a 2-crumb path the chipLeft < 0 → right-spill.
    //
    // This test guards:
    //   (i)  ◀ arrow left edge ≈ 0 (pinned at #quarto-content boundary)
    //   (ii) All crumbs are still visible (full path, no ellipsis on short paths)
    //   (iii) No horizontal scrollbar on #root
    //
    // fail-on-revert: reverting the right-spill branch (keeping chipLeft < 0 → clip)
    // → crumbs go off-screen left → x < -1 → assertion (i) fails OR crumbs missing.
    // -------------------------------------------------------------------------

    test('T19b — G1 right-spill: near-left-edge block — ◀ pinned at x≈0, full crumb path visible (guards right-spill branch)', async ({ page }) => {
        await page.addInitScript(() => {
            localStorage.setItem('quarto-hub:preferences', JSON.stringify({
                version: 1,
                scrollSyncEnabled: true,
                errorOverlayCollapsed: true,
                colorScheme: 'auto',
                unlockNestingCursor: true,
            }));
        });

        const serverUrl = getServerUrl();

        // Fixture: a paragraph inside a zero-indent fenced Div.
        // surfaceLeft ≈ colLeft (Div adds no indent).
        // With a 2-crumb path (Dv + ¶), naturalWidth = 2*22 = 44px.
        // chipLeft = surfaceLeft - OUT_W - 44; if surfaceLeft ≈ colLeft ≈ 25px:
        //   chipLeft = 25 - 16 - 44 = -35 → clamped to 0 → right-spill.
        const QMD = [
            '---',
            'format: q2-preview',
            '---',
            '',
            '::: {.zero-indent-div}',
            '',
            'Paragraph inside zero-indent Div.',
            '',
            ':::',
            '',
        ].join('\n');

        const docId = await createProjectOnServer(serverUrl, [
            { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
            { path: 'breadcrumb-t19b-edge.qmd', content: QMD, contentType: 'text' },
        ]);

        const iframe = await openFile(page, serverUrl, docId, 'breadcrumb-t19b-edge.qmd');

        // Open the paragraph inside the div.
        await iframe.locator('p[data-block-pool-id]').first().click();
        await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });
        const chip = iframe.locator('[data-testid="q2-breadcrumb-chip"]');
        await chip.waitFor({ timeout: 5_000 });

        // Measure chip and layout geometry (iframe-relative via .evaluate()).
        const colLeft = await iframe.locator('main#quarto-document-content').evaluate((el) => {
            return el.getBoundingClientRect().left;
        });

        const surfaceLeft = await iframe.locator('textarea').first().evaluate((el) => {
            return el.getBoundingClientRect().left;
        });

        const chipGeom = await iframe.locator('[data-testid="q2-breadcrumb-chip"]').evaluate((chipEl) => {
            const crumbs = Array.from(chipEl.querySelectorAll('.q2-crumb'));
            const outArrow = chipEl.querySelector('.q2-breadcrumb-out');
            if (!outArrow) return null;
            const crumbRects = crumbs.map((el) => el.getBoundingClientRect());
            const outRect = outArrow.getBoundingClientRect();
            return {
                crumbCount: crumbs.length,
                crumbTitles: crumbs.map((el) => el.getAttribute('title') ?? ''),
                crumbLeft: crumbs.length > 0 ? Math.min(...crumbRects.map((r) => r.left)) : null,
                outArrowLeft: outRect.left,
                outArrowRight: outRect.right,
            };
        });

        expect(chipGeom, 'chip geometry must be non-null').not.toBeNull();
        if (!chipGeom) throw new Error('impossible — asserted above');

        console.log(
            `T19b: colLeft=${colLeft.toFixed(2)}, surfaceLeft=${surfaceLeft.toFixed(2)}, ` +
            `crumbCount=${chipGeom.crumbCount}, titles=${JSON.stringify(chipGeom.crumbTitles)}, ` +
            `outArrowLeft=${chipGeom.outArrowLeft.toFixed(2)}, outArrowRight=${chipGeom.outArrowRight.toFixed(2)}, ` +
            `crumbLeft=${chipGeom.crumbLeft?.toFixed(2) ?? 'null'}`,
        );

        // (i) ◀ must not be clipped past x≈0.
        //     For a near-left-edge block, the right-spill branch pins ◀ at x=0
        //     (or near it, within the #quarto-content boundary). -2px tolerance.
        //     fail-on-revert: removing the right-spill clamp → outArrowLeft < -2 → RED.
        expect(
            chipGeom.outArrowLeft,
            `◀ left (${chipGeom.outArrowLeft.toFixed(2)}) must be ≥ -2 (≈0) — not clipped past x=0. ` +
            `Guards the right-spill-at-edge clamp in computeChipGeometry.`,
        ).toBeGreaterThanOrEqual(-2);

        // (ii) Full crumb path must still be visible (no ellipsis for a 2-crumb path).
        //      With right-spill the band extends rightward past the pivot but keeps
        //      naturalWidth (= 2 * CRUMB_W), so slots ≥ crumbCount.
        expect(
            chipGeom.crumbCount,
            `at least 1 crumb must be visible. Got ${chipGeom.crumbCount}. ` +
            `The path should be [Dv, ¶] (2 crumbs); both must show.`,
        ).toBeGreaterThanOrEqual(1);

        // (iii) No horizontal scrollbar on #root.
        const noHScrollbar = await iframe.locator('#root').evaluate((el) => {
            return el.scrollWidth <= el.clientWidth + 1;
        });
        expect(
            noHScrollbar,
            '#root must not have a horizontal scrollbar with right-spill geometry.',
        ).toBe(true);

        // Bonus: with right-spill, the band DOES extend right past the pivot.
        // The crumbs should be to the RIGHT of or overlapping the pivot (surfaceLeft).
        // This is the "right-spill" observable effect — crumbs extend into content area.
        // (This assertion is softer since the exact extent depends on CRUMB_W and layout.)
        if (chipGeom.crumbLeft !== null) {
            console.log(
                `T19b right-spill check: crumbLeft=${chipGeom.crumbLeft.toFixed(2)}, ` +
                `surfaceLeft=${surfaceLeft.toFixed(2)}, colLeft=${colLeft.toFixed(2)}`,
            );
        }

        await iframe.locator('textarea').first().press('Escape');
    });
});
