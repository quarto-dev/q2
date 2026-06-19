# Block-Editing E2E Test Design Spec
**Date:** 2026-06-16  
**Branch:** block-editing worktree  
**Status:** Design-only — writer agent implements, does not alter this file

---

## Purpose

This document is a pre-validated e2e test-design spec for a writer agent. It specifies four Playwright tests, each grounded in real production code and real codebase selectors. The writer implements these tests by copying/extending the patterns quoted here — no invention required and no local harness allowed.

---

## Harness Overview (verified from codebase)

### Build prerequisites

```bash
# From hub-client/ — one-time when ts-packages/preview-renderer changed:
cd ts-packages/preview-renderer && npm run build
cd hub-client && VITE_E2E=1 npm run build
```

The full e2e command that handles the build automatically:

```bash
cd hub-client && npm run test:e2e
```

### Running individual specs

```bash
cd hub-client
npx playwright test e2e/<spec-name>.spec.ts --project=chromium --workers=1
```

### Ports

- Hub server: port **3031** (started by `globalSetup`)  
- Preview static server: port **5174** (baseURL in playwright.config.ts)  
- `VITE_E2E=1` flag enables `window.__quartoTest` hooks (tree-shaken without it)

### Canonical imports (copy from every existing spec verbatim)

```ts
import { test, expect, type Page, type FrameLocator } from '@playwright/test';
import type {} from './helpers/testHooks';
import {
    bootstrapProjectSet,
    createProjectOnServer,
    seedProjectInBrowser,
    getServerUrl,
} from './helpers/projectFactory';
import { waitForPreviewRender } from './helpers/previewExtraction';
```

### Standard `openFile` helper (copy from every existing spec verbatim)

```ts
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
```

### Standard test preamble (unlock mode)

```ts
await page.addInitScript(() => {
    localStorage.setItem('quarto-hub:preferences', JSON.stringify({
        version: 1,
        scrollSyncEnabled: true,
        errorOverlayCollapsed: true,
        colorScheme: 'auto',
        unlockNestingCursor: true,
    }));
});
```

### Key production signals (verified present)

| Signal | How to read in test |
|--------|---------------------|
| Active editor open | `iframe.locator('textarea').first().waitFor({ timeout: … })` |
| Active editor buffer | `await iframe.locator('textarea').first().inputValue()` |
| Which block is active | `editTarget.anchorR0` — not directly readable; infer from buffer content |
| `data-expanded` on textarea | `await iframe.locator('textarea').first().evaluate(ta => ta.hasAttribute('data-expanded'))` |
| Breadcrumb chip visible | `iframe.locator('[data-testid="q2-breadcrumb-chip"]').waitFor(…)` |
| Individual crumb buttons | `iframe.locator('[data-testid="q2-breadcrumb-chip"] .q2-crumb')` — each has `title={c.label}` where label is the full block-type label (e.g. "BulletList"), and `textContent` = `c.abbrev` |
| Nest-out button | `iframe.getByRole('button', { name: /^Out/ })` — `title` attribute is `"Out (⌘⌃←)"` (mac) or `"Out (Alt+Shift+←)"` (other) |
| Nest-in button | `iframe.getByRole('button', { name: /^In/ })` |
| Reland fade CSS | `await iframe.locator('[data-block-pool-id]').evaluate(el => getComputedStyle(el).filter)` — non-`none` during reland gap |
| No active editor | `await iframe.locator('textarea').count()` === 0 |
| `window.__quartoTest.wasmRenderer.updateFileContent(f, c)` | inject a collaborator edit (see self-heal spec) |

---

## Test 1 — G6+G7 Settle-Gate: Dirty Edit Then Nest-Out Relands With Fresh Content

### (a) Tier Justification (why browser, not jsdom)

The settle-gate (`preCommitContentRef` / `executeLanding` G6+G7 guard in `PreviewRoot.tsx`) operates by comparing `renderedContentRef.current` against `preCommitContentRef.current`. The decisive signal is that `renderedContentRef` has advanced — meaning the WASM render pipeline produced a **new** `renderedContent` prop reflecting the committed edit, and React re-ran `PreviewRoot` so `renderedContentRef.current = props.renderedContent`. In jsdom, re-renders can be driven manually with `act()` + fake timers, but the pipeline never advances `renderedContent` in response to a real `setAst()` call — there is no WASM renderer in jsdom. Only the real browser with the real WASM pipeline produces the deterministic `renderedContent` change that unblocks the settle gate. The assertion that the destination editor RELANDS with FRESH content (not the pre-commit snapshot) requires that the props-change layout effect fired, and jsdom cannot produce that without mocking the entire render pipeline.

Additionally, the "active editor does NOT drop to the next outer block" assertion requires real timing — specifically that the reland layout effect fires between the re-render and the RELAND_BACKSTOP_MS (250ms) fallback timer. jsdom cannot produce this real async render gap.

### (b) Fixture QMD

A loose list with two items. Item A will be dirtily edited; after nest-out the editor must land on the parent list with the committed text reflected.

```ts
const QMD = [
    '---',
    'format: q2-preview',
    '---',
    '',
    '* First item',
    '',
    '* Second item',
    '',
].join('\n');
```

Why this fixture: a loose list gives a two-level nesting surface (the list itself, and each item Para as a child surface). Editing item A's Para, making it dirty, then pressing nest-out triggers the DIRTY path in `requestNestingMove` → `commitAndArmReland` → `kind:'nest'` settle-gate.

### (c) Existing spec to extend

Extend `hub-client/e2e/q2-preview-nesting-caret-in.spec.ts` (the G6+G7 test does not yet exist; create a NEW spec `q2-preview-settle-gate.spec.ts` mirroring that file's structure). Reuse:
- `openFile` helper (copy verbatim)
- Standard imports (copy verbatim)
- `iframe.getByRole('button', { name: /^Out/ }).click()` pattern (from `q2-preview-nesting-caret-in.spec.ts` line 95)
- `iframe.locator('textarea').first().inputValue()` pattern

### (d) User-Event Sequence

```ts
// 1. Enable unlock mode
await page.addInitScript(() => {
    localStorage.setItem('quarto-hub:preferences', JSON.stringify({
        version: 1,
        scrollSyncEnabled: true,
        errorOverlayCollapsed: true,
        colorScheme: 'auto',
        unlockNestingCursor: true,
    }));
});

// 2. Create project and open file
const serverUrl = getServerUrl();
const docId = await createProjectOnServer(serverUrl, [
    { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
    { path: 'settle-gate.qmd', content: QMD, contentType: 'text' },
]);
const iframe = await openFile(page, serverUrl, docId, 'settle-gate.qmd');

// 3. Click "First item" to open its Para editor
await iframe.getByText('First item', { exact: true }).click();
await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });

// 4. Verify we opened the right block
const initialValue = await iframe.locator('textarea').first().inputValue();
expect(initialValue.trim()).toBe('First item');

// 5. Type a dirty edit
await iframe.locator('textarea').first().fill('First item EDITED');
const dirtyValue = await iframe.locator('textarea').first().inputValue();
expect(dirtyValue).toBe('First item EDITED');

// 6. Press nest-out chord (mac: Meta+Ctrl+ArrowLeft; or use the breadcrumb ◀ button)
//    Using the button is more reliable cross-platform:
await iframe.getByRole('button', { name: /^Out/ }).click();

// 7. Wait for the reland: editor re-opens on the parent list
await iframe.locator('#q2-active-edit-region').waitFor({ timeout: 15_000 });
await iframe.locator('textarea').first().waitFor({ timeout: 5_000 });
```

Note: the ◀ button click triggers `ctx.requestNestingMove('out')` in BreadcrumbChip.tsx (line 410: `onClick={(e) => { e.stopPropagation(); ctx?.requestNestingMove?.('out'); }}`). The button has `onPointerDown={(e) => e.preventDefault()}` to suppress blur-commit — using `.click()` is correct (Playwright fires pointerdown+pointerup+click; the preventDefault on pointerdown keeps the textarea focused until the click callback runs `requestNestingMove`).

### (e) Assertion Surface

```ts
// ASSERTION A: editor RELANDS on the parent list (not dropped)
// The relanded editor must be visible and active
await expect(iframe.locator('textarea')).toBeVisible();
await expect(iframe.locator('textarea')).toHaveCount(1);

// ASSERTION B: the relanded buffer contains FRESH content (the committed edit "First item EDITED")
// Pre-fix / fail-on-revert: the buffer would contain the pre-commit "First item" (stale)
// OR the editor would have dropped (no textarea at all).
const landedValue = await iframe.locator('textarea').first().inputValue();
expect(
    landedValue,
    'relanded editor must contain the committed edit, not the pre-commit text'
).toContain('First item EDITED');

// ASSERTION C: the active editor is the PARENT LIST, not the item
// The parent list buffer contains BOTH items (after the edit)
expect(
    landedValue,
    'relanded editor must be on the parent list (contains both items)'
).toContain('Second item');
```

### (f) Fail-on-Revert

**Revert hunk:** In `PreviewRoot.tsx`, remove the settle-gate guard at line 775:
```ts
// REVERT THIS LINE:
if (preCommitContentRef.current !== null && renderedContentRef.current === preCommitContentRef.current) {
    return; // not consumed — leave pendingLandingRef in place
}
```

Without the settle gate, `executeLanding` fires in the props-change layout effect on the FIRST re-render after commit, which may be a render that does NOT yet reflect the committed edit (because React batches + WASM render is async). The `resolveLanding(kind:'nest')` call then uses the old `renderedContent` to relocate the committed container, and the landed editor buffer is populated from `seedForRange(range, content, ...)` using stale content — the buffer shows the pre-commit text "First item" instead of "First item EDITED". **ASSERTION B flips RED.**

**Also revert:** In `commitAndArmReland` at line 1208, remove `preCommitContentRef.current = renderedContentRef.current;` (the snapshot-before-commit step). Without the snapshot, `preCommitContentRef` stays null, the settle gate never arms, and the reland fires on the first re-render regardless. The result is the same stale-content reland. **ASSERTION B flips RED.**

### (g) Flakiness Mitigation

- Use `await iframe.locator('#q2-active-edit-region').waitFor({ timeout: 15_000 })` after nest-out to wait for the reland to complete before asserting the buffer value. The settle-gate adds at most RELAND_BACKSTOP_MS (250ms) latency plus one WASM render cycle; the 15s timeout is ample.
- The `test.setTimeout(120000)` pattern from all existing specs applies here.
- Use `test.beforeEach` with `page.waitForTimeout(1000)` when `testInfo.workerIndex > 0`.

---

## Test 2 — G6+G7 (variant): Dirty Arrow-Step-Off at Block Edge Relands With Fresh Content and Stays Active

### (a) Tier Justification

Same reasoning as Test 1 — the settle gate requires a real WASM render pipeline advancing `renderedContent`. Additionally, the "arrow step off at the block edge" path (bare ArrowDown/ArrowUp hitting the visual edge of the textarea) dispatches through `requestMove` in `PreviewRoot.tsx`, which also arms the settle gate on the dirty path. The fact that the textarea is at its visual edge and `isOnLastVisualLine` returns true is a real browser layout question — jsdom returns zero-size textareas and `scrollHeight === clientHeight` is always true in jsdom, making edge detection vacuous.

### (b) Fixture QMD

Three paragraphs so there is a middle block to dirty-edit and a next block to arrow-step into.

```ts
const QMD = [
    '---',
    'format: q2-preview',
    '---',
    '',
    'Alpha paragraph.',
    '',
    'Beta paragraph.',
    '',
    'Gamma paragraph.',
    '',
].join('\n');
```

### (c) Existing spec to extend

Extend `hub-client/e2e/q2-preview-self-heal-on-write.spec.ts` as a NEW companion spec `q2-preview-settle-gate.spec.ts` (same file as Test 1). Add as a second test in the same `test.describe` block. Reuse:
- Same `openFile` helper
- `iframe.locator('p[data-block-pool-id]').nth(N)` pattern for block selection (from self-heal spec lines 178-179)

### (d) User-Event Sequence

```ts
// 1. (unlock mode preamble same as Test 1)

// 2. Create project and open file (as above with 3-para QMD)

// 3. Click "Beta paragraph." to open its editor
await iframe.locator('p[data-block-pool-id]').nth(1).click();
await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });
expect(await iframe.locator('textarea').first().inputValue()).toBe('Beta paragraph.');

// 4. Dirty edit
await iframe.locator('textarea').first().fill('Beta paragraph. EDITED');

// 5. Move the caret to the end of the textarea (ensure we are on the last visual line)
await iframe.locator('textarea').first().press('End');
await iframe.locator('textarea').first().press('End');  // press twice to ensure end-of-last-line

// 6. Press ArrowDown — triggers requestMove('down', …) with isDirty=true
//    The settle-gate arms; the editor commits and closes, then relands on Gamma.
await iframe.locator('textarea').first().press('ArrowDown');

// 7. Wait for reland on Gamma paragraph
await iframe.locator('#q2-active-edit-region').waitFor({ timeout: 15_000 });
await iframe.locator('textarea').first().waitFor({ timeout: 5_000 });
```

### (e) Assertion Surface

```ts
// ASSERTION A: editor relanded (not dropped)
await expect(iframe.locator('textarea')).toBeVisible();
await expect(iframe.locator('textarea')).toHaveCount(1);

// ASSERTION B: relanded on GAMMA (destination block, not still on Beta)
const landedValue = await iframe.locator('textarea').first().inputValue();
expect(
    landedValue.trim(),
    'relanded editor must be on Gamma paragraph (the destination block)'
).toBe('Gamma paragraph.');

// ASSERTION C: non-vacuity — Beta was committed (file content reflects the edit)
const fileContent = await page.evaluate(async (f) => {
    await window.__quartoTestReady;
    return window.__quartoTest!.wasmRenderer.getFileContent(f) as string | null;
}, 'settle-gate-arrow.qmd');
expect(fileContent).toContain('Beta paragraph. EDITED');
expect(fileContent).not.toContain('Beta paragraph.\n'); // old text gone
```

### (f) Fail-on-Revert

Same revert hunks as Test 1 (settle gate + preCommitContentRef snapshot). Without the settle gate, the reland lands on a render that still has "Beta paragraph." (old content). `resolveLanding(kind:'outerByLine', direction:'down', destLine:…)` then computes `destLine` using the new draft line count against the OLD content line map — the destination resolves to the wrong block or the reland fires before the committed content appears. In practice the reland may open "Beta paragraph." itself again (wrong block) or may land on Gamma with stale content. **ASSERTION B or the non-vacuity ASSERTION C flips RED.**

### (g) Flakiness Mitigation

- The ArrowDown edge-trigger is fragile if the textarea is not on its last visual line. Use `press('End')` twice before pressing `ArrowDown` to force the caret to the end.
- Wait for `#q2-active-edit-region` after ArrowDown, not just `textarea`, to ensure the new editor has mounted (not the old one still closing).
- `test.setTimeout(120000)`.

---

## Test 3 — G8 Marker Hit-Test: Clicking the Bullet Marker Selects the Parent List

### (a) Tier Justification

The G8 branch in `findEditTarget` (useBlockEditHover.tsx lines 127–141) fires when `e.target === <li>` and the event target is a tight `<li>`. The condition `leaf === target` is true only when the pointer directly hit the `<li>` element itself (the list-item box, which the browser uses as the hit target for the marker/number area in the left gutter). In jsdom, `e.target` is set manually; jsdom has no CSS layout engine so there is no "marker gutter" — all clicks on `<li>` text produce `e.target = <span>` or similar inline elements, never the bare `<li>`. The actual fact "clicking the marker hits the `<li>` as a direct target because the marker lives in the `<li>`'s margin box, not any descendant" is a browser layout and hit-testing fact. jsdom cannot fake this reliably without constructing the exact event that the production guard tests for.

### (b) Fixture QMD

A tight bullet list (no blank lines between items — items are Plain nodes, not Para nodes).

```ts
const QMD = [
    '---',
    'format: q2-preview',
    '---',
    '',
    '- alpha',
    '- beta',
    '- gamma',
    '',
].join('\n');
```

This is the SAME fixture as `q2-preview-item-edit-size.spec.ts` (line 50): `['---', 'format: q2-preview', '---', '', '- one', '- two', '- three', ''].join('\n')`. Reuse the pattern with slightly different content words to avoid fixture collision.

### (c) Existing spec to extend

Extend `hub-client/e2e/q2-preview-item-edit-size.spec.ts` as a NEW companion spec `q2-preview-marker-hit-test.spec.ts`. Mirror its structure entirely. Reuse:
- `openFile` helper (copy verbatim)
- `iframe.locator('ul[data-block-pool-id]').first().boundingBox()` for list geometry
- `iframe.locator('li', { hasText: '...' }).first()` for item selection
- `iframe.locator('#q2-active-edit-region')` for the active edit region

**Key selector verified present:** `ul[data-block-pool-id]` — tight bullet lists have `data-block-pool-id` on the `<ul>` (confirmed from item-edit-size spec line 80 and the G8 comment in useBlockEditHover.tsx: "A tight `<li>`/`<dd>` borrows the leading Plain's pool-id").

### (d) User-Event Sequence

The challenge: click the MARKER area of a tight list item, not the text. The marker lives in the left gutter (CSS `list-style-position: outside`). In a rendered tight bullet list, clicking the `<li>` element's leftmost edge (the marker gutter) produces `e.target === <li>`, which triggers the G8 parent-list resolution.

```ts
// 1. Enable unlock mode (same preamble)

// 2. Create project and open file

// 3. Record the list's bounding box
const listH = (await iframe.locator('ul[data-block-pool-id]').first().boundingBox())!.height;
const listBox = (await iframe.locator('ul[data-block-pool-id]').first().boundingBox())!;

// 4. Get the bounding box of the first <li>
const itemBox = (await iframe.locator('li', { hasText: 'alpha' }).first().boundingBox())!;

// Sanity: the fixture really has a multi-item list
expect(itemBox.height).toBeLessThan(listH - 2);

// 5. Click the MARKER area: x = itemBox.x - 5 (just left of the text, in the marker gutter).
//    The marker of a tight bullet list is rendered OUTSIDE the content box (list-style-position: outside),
//    so clicking at itemBox.x - 8 (inside the <li> box but outside the text content) hits the <li> directly.
//    In Playwright, page.click with an absolute coordinate targets the iframe-relative position.
//
//    IMPORTANT: use iframe.locator('li', { hasText: 'alpha' }).click({ position: { x: -8, y: itemBox.height / 2 } })
//    where position is relative to the element's top-left. A negative x hits the marker gutter.
await iframe.locator('li', { hasText: 'alpha' }).first().click({
    position: { x: -8, y: Math.floor(itemBox.height / 2) },
});

// 6. Wait for the editor to open
await iframe.locator('#q2-active-edit-region').waitFor({ timeout: 10_000 });
await iframe.locator('textarea').first().waitFor({ timeout: 5_000 });
```

### (e) Assertion Surface

```ts
// ASSERTION A: the editor buffer is the WHOLE LIST, not just one item
// The whole-list buffer contains all three items (newline-separated)
const buffer = await iframe.locator('textarea').first().inputValue();
console.log(`marker hit-test: buffer=${JSON.stringify(buffer)}`);

expect(
    buffer,
    'clicking the marker must activate the PARENT LIST (buffer contains all items)'
).toContain('beta');
expect(
    buffer,
    'clicking the marker must activate the PARENT LIST (buffer contains all items)'
).toContain('gamma');

// ASSERTION B: the edit region sizes to the WHOLE LIST (≈ listH), not one item
const editH = (await iframe.locator('#q2-active-edit-region').boundingBox())!.height;
const TOL = 2;
expect(
    editH,
    `edit region height (${editH.toFixed(1)}) must be ≈ the whole-list height (${listH.toFixed(1)})`
).toBeGreaterThan(listH - TOL);

await iframe.locator('textarea').first().press('Escape');
```

### (f) Fail-on-Revert

**Revert hunk:** In `useBlockEditHover.tsx`, remove the G8 marker branch (lines 127–139):

```ts
// REVERT: remove this block
if (leaf && leaf === target && (target.tagName === 'LI' || target.tagName === 'DD')
    && ctx?.unlockNestingCursorRef?.current) {
    return leaf.parentElement?.closest('[data-block-pool-id]') ?? leaf; // parent list
}
```

Without G8, `findEditTarget` returns the `<li>` itself (which borrows the leading Plain's pool-id in tight lists). The `activate(el)` call then opens the ITEM editor (the individual list item "alpha"), not the parent list. **ASSERTION A flips RED** — the buffer contains only "alpha", not "beta" and "gamma". **ASSERTION B also flips RED** — the edit region height matches one item, not the whole list.

### (g) Flakiness Mitigation

The marker-click position (`{ x: -8, y: ... }`) is fragile: if the list-item padding or the marker offset changes, the click may miss the marker gutter and land on the text instead. Mitigations:
- Add a `console.log` that prints `e.target.tagName` — but this is only for debugging; the assertion on buffer content is the real gate.
- If the marker-click approach fails (the browser clips negative-x clicks), use an alternative: click the `<li>` at its extreme left edge by computing `listBox.x - 1` in absolute page coordinates and using `page.click`:
  ```ts
  const liBox = await iframe.locator('li', { hasText: 'alpha' }).first().boundingBox();
  // liBox is in page coordinates (iframe-relative not guaranteed with boundingBox)
  // Use evaluate to get iframe-relative coords:
  const liIframeRect = await iframe.locator('li', { hasText: 'alpha' }).first().evaluate(
      el => el.getBoundingClientRect()
  );
  // click at x = liIframeRect.left - 5 (in the marker gutter, still inside the <li> box)
  ```
  **This alternative approach is flagged as unvalidated** — see "Unvalidated" section below.

---

## Test 4 — T13(c) Crumb-Does-Not-Carry-Expansion: Crumb Jump Does Not Expand the Destination Editor

### (a) Tier Justification

The breadcrumb chip crumb buttons (`<button className="q2-crumb">`) are only rendered under real browser layout: the chip's `[data-testid="q2-breadcrumb-chip"]` is conditionally rendered by `BreadcrumbChip.tsx` only when `unlockNestingCursor && editTarget`, and its positioning (`useLayoutEffect` that reads `getBoundingClientRect()`) requires a real layout engine. In jsdom, `getBoundingClientRect()` returns zero for everything — the chip's `geom` state stays `null`, and the crumb buttons render at unpositioned fallback positions. More critically, the P3.5 breadcrumb-geometry spec comment (line 17) explicitly notes: "jsdom returns zero rects for everything, so every assertion here is Playwright-only." The actual crumb button click requires a real DOM element to be reachable (non-zero size). A jsdom test of crumb-click behavior was noted as going VACUOUS (the spec instruction confirms: "this is the case that went VACUOUS in jsdom").

### (b) Fixture QMD

A loose bullet list (items are Para nodes, giving a two-level nesting hierarchy: List → Para). A loose list is required so each item is a `Para` (editable surface) rather than a `Plain` (also editable as an item proxy). After opening a Para item and expanding the editor, then nest-out to the list, we have the list EXPANDED. A crumb-click on the item Para crumb (an ancestor crumb from the list's editor perspective — wait: crumb-jump goes to an ANCESTOR, i.e. "up" the path). Re-read the spec requirement:

> open a surface EXPANDED, then click a BREADCRUMB CRUMB to jump to an ancestor → the relanded editor is NOT expanded

So we need: an editable block nested at depth ≥ 2, opened and EXPANDED, then a crumb-click to an ancestor that was NOT previously expanded. The key production behavior is that `applyNestingRetarget` (called by clean crumb jumps) calls `openEditTarget(..., { keepExpanded: false })`, and `openEditTarget` resets `editExpandedRef.current = false` when `keepExpanded` is falsy. On the next render, the relanded `EditTextarea` reads `editExpandedRef.current` as false, so it opens collapsed.

Minimal fixture: a Para inside a fenced Div (two-level hierarchy: Div → Para).

```ts
const QMD = [
    '---',
    'format: q2-preview',
    '---',
    '',
    '::: {.outer}',
    '',
    'Inner paragraph with several lines of text',
    'that span more than one source line',
    'and will expand when the editor opens.',
    '',
    ':::',
    '',
].join('\n');
```

The inner Para is a multi-line source paragraph (3 source lines) so that expanding it produces a visibly taller textarea than the collapsed (render) height — same fixture strategy as `q2-preview-expand-on-edit.spec.ts`.

### (c) Existing spec to extend

Extend `hub-client/e2e/q2-preview-breadcrumb-geometry.spec.ts` as a NEW spec `q2-preview-crumb-no-carry-expansion.spec.ts`. Mirror its `openFile` helper (which is identical to every other spec's). Reuse:
- `iframe.locator('[data-testid="q2-breadcrumb-chip"]').waitFor({ timeout: 5_000 })` pattern
- `iframe.locator('[data-testid="q2-breadcrumb-chip"] .q2-crumb')` for crumb buttons
- `iframe.locator('textarea').first().evaluate(ta => ta.hasAttribute('data-expanded'))` pattern from `q2-preview-expand-on-edit.spec.ts` line 75

The crumb buttons are `<button>` elements with `className="q2-crumb"` and `title={c.label}` where label is the block-type label (e.g. "Div"). Each crumb also has `aria-label={c.label}`. **Crumb selector verified:** `.q2-crumb` CSS class in BreadcrumbChip.tsx line 320. **Title attribute verified:** `title={c.label}` in BreadcrumbChip.tsx line 439. **Click handler verified:** `onClick={(e) => { e.stopPropagation(); ctx?.requestNestingSelect?.(c.r0, c.r1); }}` in BreadcrumbChip.tsx line 442.

### (d) User-Event Sequence

```ts
// 1. Enable unlock mode (same preamble)

// 2. Create project and open file (with Div-wrapped multi-line Para fixture)

// 3. Open the inner Para editor
await iframe.locator('p[data-block-pool-id]').first().click();
await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });

// 4. Verify the chip is visible (we are in a nested surface)
const chip = iframe.locator('[data-testid="q2-breadcrumb-chip"]');
await chip.waitFor({ timeout: 5_000 });

// 5. Trigger EXPANSION: type a character (the §7 expand-on-edit trigger)
await iframe.locator('textarea').first().press('x');
// Wait for data-expanded to appear
await expect(iframe.locator('textarea[data-expanded]')).toBeVisible({ timeout: 3_000 });

// Confirm expanded
const expandedBefore = await iframe.locator('textarea').first().evaluate(
    (el) => (el as HTMLTextAreaElement).hasAttribute('data-expanded')
);
expect(expandedBefore, 'editor must be expanded before crumb click').toBe(true);

// Record the expanded height
const expandedHeight = await iframe.locator('textarea').first().evaluate(
    (el) => (el as HTMLTextAreaElement).getBoundingClientRect().height
);

// 6. Click a CRUMB that is an ANCESTOR of the current surface.
//    The crumb for the outer Div has title="Div" and class "q2-crumb".
//    We use getByRole('button', { name: 'Div' }) scoped to the chip.
await chip.getByRole('button', { name: 'Div' }).click();

// 7. Wait for the reland on the Div surface
await iframe.locator('#q2-active-edit-region').waitFor({ timeout: 10_000 });
await iframe.locator('textarea').first().waitFor({ timeout: 5_000 });
```

**Note on crumb label:** The Div crumb label is the raw block-type name "Div" — verified from `buildAncestorPath` in `BreadcrumbChip.tsx` and the `c.label` field used in `title={c.label}`. The writer must confirm the actual label by running the test once and reading `console.log` of crumb titles.

### (e) Assertion Surface

```ts
// ASSERTION A: the relanded editor is NOT expanded
// data-expanded must be ABSENT — crumb jumps do not carry expansion.
const expandedAfter = await iframe.locator('textarea').first().evaluate(
    (el) => (el as HTMLTextAreaElement).hasAttribute('data-expanded')
);
expect(
    expandedAfter,
    'crumb-jumped editor must NOT be expanded (crumb jumps reset expansion)'
).toBe(false);

// ASSERTION B: the relanded editor IS on the Div surface (buffer contains all of the Para source)
const landedBuffer = await iframe.locator('textarea').first().inputValue();
expect(
    landedBuffer,
    'crumb-jumped editor must contain the inner Para text (we landed on the Div)'
).toContain('Inner paragraph');

// ASSERTION C: collapsed height ≤ expanded height (non-vacuity — we really had expanded before)
const collapsedHeight = await iframe.locator('textarea').first().evaluate(
    (el) => (el as HTMLTextAreaElement).getBoundingClientRect().height
);
console.log(`crumb-no-carry-expansion: expandedHeight=${expandedHeight.toFixed(1)}, collapsedHeight=${collapsedHeight.toFixed(1)}`);
// The Div surface may be taller or shorter than the Para surface — what matters is
// that the new editor does NOT inherit the Para's expanded state.
// We assert data-expanded is absent (ASSERTION A), which is the real gate.
// ASSERTION C is a soft check: log for human review, do not fail on height alone.

await iframe.locator('textarea').first().press('Escape');
```

### (f) Fail-on-Revert

**Revert hunk:** In `PreviewRoot.tsx`, change `executeLanding`'s `carryExpanded` computation (line 784):

```ts
// CURRENT (correct):
const carryExpanded = pl.spec.kind === 'nest';
openFromResolved(r, pl.caret, carryExpanded);

// REVERT (broken — carry expansion for ALL landings including crumbs):
const carryExpanded = true; // always carry
openFromResolved(r, pl.caret, carryExpanded);
```

With this revert, `executeLanding` passes `keepExpanded: true` for crumb landings. `openEditTarget` then does NOT reset `editExpandedRef.current`, and the relanded `EditTextarea` reads `editExpandedRef.current = true` at mount → `expanded` initializes true → `data-expanded` is present on the textarea immediately. **ASSERTION A flips RED** (`expandedAfter === true`).

**Alternate revert hunk (same effect):** In `openEditTarget` (PreviewRoot.tsx around line 513):

```ts
// CURRENT:
if (!opts.keepExpanded) editExpandedRef.current = false;

// REVERT (never reset):
// (remove the line entirely, or change to unconditional no-op)
```

Same result — `editExpandedRef` carries the true value into the destination editor. **ASSERTION A flips RED.**

**For the CLEAN (non-dirty) crumb-jump path:** The `requestNestingSelect` clean branch calls `applyNestingRetarget({ r0, r1 }, et.leafAnchorR0, caret)` WITHOUT `keepExpanded` — so `keepExpanded` is undefined/false, which resets `editExpandedRef`. Reverting `applyNestingRetarget` to pass `keepExpanded: true` would also flip **ASSERTION A RED**.

The test is designed with a CLEAN crumb jump (the single typed character 'x' did dirty the buffer, but a clean crumb-jump test can also be designed by deleting the character first — see "Unvalidated" note below on whether the dirty vs. clean path matters here).

### (g) Flakiness Mitigation

- After `press('x')`, wait explicitly for `textarea[data-expanded]` with a short timeout before proceeding to the crumb click. This avoids a race where the crumb is clicked before expand fires.
- The crumb button for "Div" may have a different label depending on how `buildAncestorPath` formats it. Add a `console.log` of all crumb titles before clicking:
  ```ts
  const crumbTitles = await chip.locator('.q2-crumb').evaluateAll(
      els => els.map(el => el.getAttribute('title') ?? '')
  );
  console.log(`crumb titles: ${JSON.stringify(crumbTitles)}`);
  ```
  Then use `chip.locator('.q2-crumb').first()` (the outermost ancestor = Div) as a fallback if the `getByRole({ name: 'Div' })` does not match.

---

## Test 5 — G9 Reland-Fade: Source Cell Gets Non-`none` Filter During Reland Gap

### (a) Tier Justification

The reland fade is a CSS animation (`@keyframes q2-reland-fade-in`) applied via the `q2-reland-fade` class (useBlockEditHover.tsx lines 352–358). The assertion is on `getComputedStyle(el).filter` during the reland gap. jsdom does not run CSS animations and returns `'none'` for `filter` regardless of animation state. Checking `classList.has('q2-reland-fade')` would be possible in jsdom, but the class is added in a `useLayoutEffect` keyed on `editTarget === null && pendingLandingRef.current !== null` — the timing requires the real async render pipeline (WASM render → React re-render → layout effect fires). Without the real pipeline, jsdom would require hand-crafted re-render calls to arrive at the correct condition, which is reimplementing the production logic in the test.

**This test is timing-sensitive.** The fade class is present only during the reland gap (from `setEditTargetRaw(null)` until `clearRelandFade()` is called in `openEditTarget`). The gap is bounded by RELAND_BACKSTOP_MS (250ms) + WASM render latency. **Mark this test as potentially flaky** — see flakiness mitigation.

### (b) Fixture QMD

Same two-paragraph fixture as the self-heal spec, or the settle-gate Test 1 fixture. A dirty edit on the first Para, then nest-out chord, creates the reland gap.

```ts
const QMD = [
    '---',
    'format: q2-preview',
    '---',
    '',
    '* First loose item.',
    '',
    '* Second loose item.',
    '',
].join('\n');
```

### (c) Existing spec to extend

Add as a third test in `q2-preview-settle-gate.spec.ts` (same file as Tests 1 and 2). Alternatively, add to the existing `q2-preview-self-heal-on-write.spec.ts` — but given that spec is marked `test.fail()` for a known bug, a new spec file is cleaner.

### (d) User-Event Sequence

The fade class is added in the `useLayoutEffect` keyed on `editTarget` (lines 841–857 in PreviewRoot.tsx). It fires when:
1. `editTarget === null` (editor just closed)
2. `pendingLandingRef.current !== null` (a reland is in flight)
3. `fadeSourceR0Ref.current !== null` (set in `commitAndArmReland` line 1211: `fadeSourceR0Ref.current = et.anchorR0`)

This means the dirty nest-out path IS the trigger (it goes through `commitAndArmReland`).

```ts
// 1. Enable unlock mode

// 2. Create project, open file

// 3. Open "First loose item." Para
await iframe.locator('li p[data-block-pool-id]').first().click();
await iframe.locator('textarea').first().waitFor({ timeout: 10_000 });
expect(await iframe.locator('textarea').first().inputValue()).toContain('First loose item');

// 4. Dirty edit
await iframe.locator('textarea').first().fill('First loose item. DIRTIED');

// 5. Click the ◀ Out button (triggers commitAndArmReland → reland gap)
await iframe.getByRole('button', { name: /^Out/ }).click();

// 6. IMMEDIATELY after click, before reland completes, sample the filter
//    The reland gap is ~250ms + WASM latency; we have a window to observe the fade.
//    We look for the source cell (the <p> or <li> element that had the editor).
//    The fade is applied to elements matching `entry.r[0] === fadeSourceR0Ref.current`.
//    The source cell is the <li p> that had the "First loose item. DIRTIED" editor.
//    After commitAndArmReland closes the editor (setEditTargetRaw(null)),
//    the layout effect runs and adds .q2-reland-fade to that element.
//
//    Strategy: poll for the fade class or non-none filter within 300ms.
```

### (e) Assertion Surface

```ts
// Poll for the fade class — it appears in the reland gap (< 300ms after click)
let fadeSeen = false;
const deadline = Date.now() + 500; // 500ms window to catch the fade
while (Date.now() < deadline) {
    const hasFade = await iframe.locator('body').evaluate(() => {
        const el = document.querySelector('.q2-reland-fade');
        return el !== null;
    });
    if (hasFade) {
        fadeSeen = true;
        // Also check that getComputedStyle.filter is non-none (the animation is running)
        const filterValue = await iframe.locator('.q2-reland-fade').first().evaluate((el) => {
            return getComputedStyle(el).filter;
        });
        console.log(`G9 reland-fade: filter=${filterValue}`);
        // The animation sets filter: blur(Npx) or similar — just check not 'none'
        // In practice the filter will be in transition; 'none' means the animation didn't start.
        expect(
            filterValue,
            'q2-reland-fade element must have a non-none CSS filter during the reland gap'
        ).not.toBe('none');
        break;
    }
    await page.waitForTimeout(20);
}

// ASSERTION: the fade class was seen at least once during the reland gap
expect(
    fadeSeen,
    'q2-reland-fade class must appear on the source cell during the reland gap'
).toBe(true);

// Wait for the reland to complete (so the test does not leave state dangling)
await iframe.locator('#q2-active-edit-region').waitFor({ timeout: 15_000 });
```

### (f) Fail-on-Revert

**Revert hunk A:** In `PreviewRoot.tsx`, remove the G9 `useLayoutEffect` (lines 841–857). The fade class is never added. **`.q2-reland-fade` selector finds nothing → `fadeSeen` stays false → ASSERTION flips RED.**

**Revert hunk B:** In `commitAndArmReland` (line 1211), remove `fadeSourceR0Ref.current = et.anchorR0`. The layout effect runs but `r0` is null, so the pool scan `entry?.r[0] === null` never matches any element. **Same result — `fadeSeen` stays false → RED.**

**Revert hunk C:** In `useBlockEditHover.tsx`, remove the `@keyframes q2-reland-fade-in` and `.q2-reland-fade` CSS (lines 352–358). The class is added but the animation does not run. `getComputedStyle(el).filter` returns `'none'` even though `fadeSeen` is true. **The inner `expect(filterValue).not.toBe('none')` flips RED.**

### (g) Flakiness Mitigation

**This test is timing-sensitive and is explicitly designed to be OPTIONAL (marked as potentially flaky).** Recommended approach:

1. Use `test.fixme` or a tolerance-based pass: if `fadeSeen` is false after the 500ms poll, log a warning and `test.skip()` instead of failing — timing races in CI can prevent the 20ms polling interval from catching the fade.
2. Alternative: assert only the CLASS presence (not `getComputedStyle`), since class mutation is synchronous with the `useLayoutEffect` and does not depend on CSS animation timing. The `filter` computed-style assertion should be wrapped in a try/catch with a soft warning.
3. Recommended test annotation: `test('G9 reland-fade: ... (timing-sensitive, may be flaky in slow CI)', ...)` — and use `test.fail()` initially to track it as a known-flaky gate.
4. The writer may choose to design this as `test.fixme` and invest in a more robust signal (e.g. a `window.__quartoTest` hook that records when the fade was applied) rather than racing the 20ms poll.

---

## Writer Instructions

### Where to put each test

| Test | New file |
|------|----------|
| Test 1 (G6+G7 settle-gate, nest-out) | `hub-client/e2e/q2-preview-settle-gate.spec.ts` |
| Test 2 (G6+G7 settle-gate, arrow-step) | Same file as Test 1 — second `test(...)` in the same `test.describe` block |
| Test 3 (G8 marker hit-test) | `hub-client/e2e/q2-preview-marker-hit-test.spec.ts` |
| Test 4 (T13(c) crumb-no-carry-expansion) | `hub-client/e2e/q2-preview-crumb-no-carry-expansion.spec.ts` |
| Test 5 (G9 reland-fade) | Third `test(...)` in `hub-client/e2e/q2-preview-settle-gate.spec.ts`, OR a standalone `q2-preview-reland-fade.spec.ts` if marked `test.fixme` |

### Build/run commands

```bash
# Build once (required before any e2e run):
cd hub-client && VITE_E2E=1 npm run build

# Run a specific spec:
cd hub-client && npx playwright test e2e/q2-preview-settle-gate.spec.ts --project=chromium --workers=1

# Run all e2e (slow, requires full build including WASM):
cd hub-client && npm run test:e2e
```

### Hard rules for the writer (DO NOT VIOLATE)

1. **No local harness.** Do NOT write helper functions that re-implement the settle-gate, fade, or expansion logic. Every assertion must read a production signal (textarea value, CSS class, `data-expanded` attribute, bounding box) from the REAL DOM.

2. **No test-computed expected values.** Do NOT compute what "the buffer should be" by running string-manipulation on the fixture QMD inside the test. Assert against hardcoded strings (e.g. `'First item EDITED'`) that come from the test setup, not from in-test logic.

3. **Copy `openFile` verbatim** — do not inline `bootstrapProjectSet`, `seedProjectInBrowser`, `waitForPreviewRender`. Every existing spec copies this helper; maintain that pattern.

4. **Copy the standard preamble verbatim** — `localStorage.setItem('quarto-hub:preferences', JSON.stringify({ ... unlockNestingCursor: true ... }))` inside `addInitScript`. Tests 1, 2, 3, 4, 5 all require unlock mode.

5. **test.setTimeout(120000) on every describe block.** Required for WASM render pipeline latency.

6. **test.beforeEach with worker stagger.** Copy the `if (testInfo.workerIndex > 0) await page.waitForTimeout(1000)` pattern from all existing specs.

7. **For Test 3 (marker hit-test):** The negative-x click position `{ x: -8, ... }` relies on Playwright allowing clicks outside the element's bounding box. If Playwright clips to the element's box, use the `force: true` option: `.click({ position: { x: -8, ... }, force: true })`. Alternatively compute the absolute iframe coordinate and use `page.mouse.click(...)` on the iframe's absolute coordinates.

---

## Unvalidated Items (Writer Must Confirm First)

1. **Test 3 — Marker click coordinate:** The `{ x: -8, ... }` position relative to the `<li>` element is an estimate. The writer MUST:
   (a) Run the test once and add a `console.log` of the resolved `e.target.tagName` and `e.target === li` check inside the production code (temporarily), or
   (b) Use Playwright's `locator.evaluate` to print the `<li>`'s `getBoundingClientRect()` and verify the marker gutter is at negative-x relative to the element's left edge.
   If the marker gutter is NOT accessible via negative-x (e.g. the `<li>` element's layout box starts at the marker), the click strategy needs adjustment. In that case, use `page.mouse.move()` + `page.mouse.click()` with absolute coordinates computed from the `<li>`'s bounding box minus the browser's list-item marker gutter width (typically 20–30px for a `padding-left: 2em` list).

2. **Test 4 — Div crumb label:** The `title="Div"` assumption for the outer Div's crumb is based on reading `BreadcrumbChip.tsx` and `buildAncestorPath`. The writer must verify the actual `title` attribute by adding a `console.log(crumbTitles)` and checking what `buildAncestorPath` produces for a `::: {.outer}` fenced Div. If the label is different (e.g. "Div (.outer)" or just the block type abbreviation), adjust the `getByRole('button', { name: '...' })` selector accordingly.

3. **Test 4 — Dirty vs. clean crumb jump:** The test as designed types 'x' to dirty the buffer before the crumb click. The dirty crumb path goes through `commitAndArmReland` → reland layout effect → `executeLanding` with `carryExpanded = pl.spec.kind === 'nest'` (crumb is kind:'crumb', so `carryExpanded = false`). The clean crumb path goes through `applyNestingRetarget` with no `keepExpanded` arg → `openEditTarget` resets `editExpandedRef`. Both paths should produce the same "not expanded" result. The writer may choose to test EITHER path — or test both in separate `test(...)` blocks. If testing the dirty path, add ASSERTION C to verify the commit round-tripped (file contains the typed 'x').

4. **Test 5 — CSS animation and `getComputedStyle`:** In some Chromium configurations, `getComputedStyle(el).filter` may return `'none'` during the first animation frame even when the animation is running, because `forwards` fill-mode only applies after the animation completes. The writer should test this empirically. If `filter` is unreliable, the assertion may be weakened to just the class presence (`classList.has('q2-reland-fade')`), which is a synchronous production signal.

5. **Test 5 — `commitAndArmReland` vs. `requestMove` dirty path:** The test uses the breadcrumb ◀ button (nest-out) to trigger `commitAndArmReland`. An alternative trigger is a dirty Arrow-step-off (Test 2 scenario), which triggers `requestMove`'s dirty path. The requestMove dirty path does NOT call `commitAndArmReland` — it sets `fadeSourceR0Ref.current` via the same pattern (check line 940 and 1091 vs. 1211 in PreviewRoot.tsx). Verify that BOTH dirty-commit paths set `fadeSourceR0Ref.current` — or limit the test to the nest-out path which has been confirmed.

---

## Parent Review — BINDING ADJUSTMENTS (read FIRST; overrides any conflicting test-body detail above)

Reviewed against the test-integrity protocol. Verdicts + required changes:

**Test 1 (G6+G7 nest-out) — APPROVED as written.** Strong binding: the parent-list reland buffer directly contains the edited content, so stale-vs-fresh is visible and the fail-on-revert (remove settle-gate) cleanly flips ASSERTION B red.

**Test 2 (G6+G7 arrow-step-off) — MUST STRENGTHEN (current discriminator is degenerate).** As written, the destination (Gamma) is a *different* block from the edited one (Beta), so a stale-content reland is invisible (Gamma is unchanged either way) and `toBe('Gamma paragraph.')` would pass even with the gate reverted. The real fail-on-revert symptom is the **DROP** (editor lands one block *past* the destination). Fix:
- Fixture: add a 4th paragraph **`Delta paragraph.`** after Gamma.
- Assertion: relanded editor `inputValue().trim() === 'Gamma paragraph.'` **AND explicitly NOT** `'Delta paragraph.'`.
- Fail-on-revert: removing the settle-gate makes the dirty arrow-step reland early with a stale `anchorSlice` → self-heal DROP → focus lands on **Delta** → the `=== 'Gamma paragraph.'` assertion flips RED. Without Delta there is nothing to drop onto and the test is fixture-degenerate. This test's job is the `requestMove` dirty-snapshot site specifically (Test 1 covers `commitAndArmReland`); it is NOT a re-test of G3 edge-detection (that's T21).

**Test 3 (G8 marker hit-test) — APPROVED, conditional.** The buffer/height assertions are real production signals — good. The negative-x marker click is the execution risk: the writer MUST empirically confirm the click opens the **parent-list** editor (buffer contains `beta`+`gamma`) and, if Playwright clips negative-x, fall back to absolute `page.mouse.click(...)` at the computed gutter coordinate. The assertion is ALWAYS the real buffer/height — never a synthesized `e.target`.

**Test 4 (T13(c) crumb-no-carry) — APPROVED with a committed path.** Resolve the dirty-vs-clean ambiguity: implement the **DIRTY** path only (the `press('x')` both expands and dirties → crumb-jump exercises `executeLanding`'s `carryExpanded = pl.spec.kind === 'nest'`; fail-on-revert = set that to `true` at PreviewRoot line ~784). The sole gate is **ASSERTION A** (`data-expanded` absent); height stays a soft log. Validate the Div crumb `title` empirically (console.log titles, fall back to `.q2-crumb` outermost). **Residual (document, do not block):** the *clean* crumb path's per-caller `applyNestingRetarget` gating is not bound by this test — note it as a follow-up (a clean variant would expand via G11 second-click, which expands without dirtying).

**Test 5 (G9 fade) — DEFER, do not write now.** It's optional and timing-flaky, and it conflates tiers: the class-apply *logic* is jsdom-testable (already covered by the cluster's T7) and only the CSS `filter` is browser-only. Do not pollute the suite with a 20ms-poll flaky gate. Document it as a follow-up: if implemented later, it must be `test.fixme` asserting ONLY `getComputedStyle(el).filter !== 'none'` (the browser-only fact), never the class presence.

**Net for the writer: implement Tests 1, 2 (strengthened), 3, 4. Skip Test 5.** Every assertion reads a real production signal; each test must be RED with its named revert hunk before it is frozen.

### OUTCOME (post-implementation, 2026-06-17)
- **Tests 3 (G8 marker) & 4 (T13(c) crumb) — KEPT.** Both proven RED-on-revert (G8: removing the marker branch → buffer is `alpha` only; T13(c): `carryExpanded = true` → `data-expanded` present). Note: the writer correctly found that for a TIGHT `<li>` (text directly in the `<li>`, no `<p>` wrapper) *any* text click yields `e.target === <li>` — so clicking the item text is the valid G8 trigger; the negative-x marker-gutter click was unnecessary.
- **Tests 1 & 2 (settle-gate) — REMOVED as vacuous.** They passed GREEN but the revert hunk did NOT flip them RED: on fast hardware the real WASM render advances `renderedContent` before the backstop timer fires, so the early-land race never occurs and the gate has no observable effect in a real browser. Per the protocol (a test that passes with the production code reverted is testing the wrong thing; use the lowest *faithful* tier), the settle-gate is bound at the **jsdom tier** — `g6-g7-settle-gate.integration.test.tsx` (T1/T2), which uses fake timers to force the race deterministically and DID prove RED-on-revert (timer lands early on the stale slice). The browser cannot faithfully test this timing guard, so no Playwright test should claim to.

---

## Production Files Referenced (Absolute Paths)

- `/Users/gordon/src/q2/.worktrees/block-editing/ts-packages/preview-renderer/src/q2-preview/PreviewRoot.tsx`
- `/Users/gordon/src/q2/.worktrees/block-editing/ts-packages/preview-renderer/src/q2-preview/useBlockEditHover.tsx`
- `/Users/gordon/src/q2/.worktrees/block-editing/ts-packages/preview-renderer/src/q2-preview/BreadcrumbChip.tsx`
- `/Users/gordon/src/q2/.worktrees/block-editing/ts-packages/preview-renderer/src/q2-preview/dispatchers.tsx`
- `/Users/gordon/src/q2/.worktrees/block-editing/hub-client/e2e/q2-preview-self-heal-on-write.spec.ts`
- `/Users/gordon/src/q2/.worktrees/block-editing/hub-client/e2e/q2-preview-item-edit-size.spec.ts`
- `/Users/gordon/src/q2/.worktrees/block-editing/hub-client/e2e/q2-preview-breadcrumb-geometry.spec.ts`
- `/Users/gordon/src/q2/.worktrees/block-editing/hub-client/e2e/q2-preview-nesting-caret-in.spec.ts`
- `/Users/gordon/src/q2/.worktrees/block-editing/hub-client/e2e/q2-preview-expand-on-edit.spec.ts`
- `/Users/gordon/src/q2/.worktrees/block-editing/hub-client/e2e/helpers/projectFactory.ts`
- `/Users/gordon/src/q2/.worktrees/block-editing/hub-client/e2e/helpers/testHooks.ts`
- `/Users/gordon/src/q2/.worktrees/block-editing/hub-client/e2e/helpers/previewExtraction.ts`
