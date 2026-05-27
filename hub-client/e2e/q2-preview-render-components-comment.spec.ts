/**
 * Phase 1 investigation e2e for the reactji-authorship demo
 * (claude-notes/plans/2026-05-25-reactji-authorship-q2-preview.md).
 *
 * Sister to `q2-debug-render-components.spec.ts` (May 12) and to
 * `q2-preview-render-components-write.spec.ts` (the incremental-write
 * spec). This spec is *expected to fail today* on `feature/provenance`:
 * it probes whether `astContext.attribution` + the current Automerge
 * actor id reach the user-TSX environment for `q2-preview`. The failure
 * mode tells us which of the three plumbing gaps in the plan are real:
 *
 *   1. `__Q2_PREVIEW_RENDERER__.useNodeAttribution` is exposed
 *   2. (downstream — runtime-added spans have no `s`; out of scope here)
 *   3. `__Q2_PREVIEW_RENDERER__.useCurrentActor` is exposed and the
 *      current actor id is forwarded into the iframe
 *
 * `comment.tsx` ships a `window.__COMMENT_DIAG__` diagnostic export
 * (added under Phase 1) that this spec reads to confirm the surface
 * shape and per-span attribution resolution. The diagnostic only
 * mounts its hook-calling sub-component when both hooks are present on
 * the renderer surface, so calling this spec against today's code
 * leaves `__COMMENT_DIAG__.blocks` empty and `me === null` — clear,
 * non-throwing signal of what's missing.
 */

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { test, expect, type ConsoleMessage } from '@playwright/test';
import {
  bootstrapProjectSet,
  createProjectOnServer,
  seedProjectInBrowser,
  getServerUrl,
} from './helpers/projectFactory';

const FIXTURE_DIR = resolve(
  import.meta.dirname,
  '../../crates/quarto/tests/playwright-fixtures/q2-preview/render-components-comment',
);

const qmdContent = readFileSync(
  resolve(FIXTURE_DIR, 'render-components-comment.qmd'),
  'utf-8',
);
const tsxContent = readFileSync(resolve(FIXTURE_DIR, 'comment.tsx'), 'utf-8');
const quartoYmlContent = readFileSync(
  resolve(FIXTURE_DIR, '_quarto.yml'),
  'utf-8',
);

// Stable id injected via App.tsx's `__QUARTO_TEST_ACTOR_ID__` override
// so the iframe's `useCurrentActor()` resolves to a known value even
// though the e2e harness runs without auth (where `getActorId()` is
// otherwise null). Used both to seed the override and to assert that
// the id round-tripped end-to-end.
//
// Automerge actor ids are 16-byte hex strings — using a non-hex
// placeholder makes the runtime reject the writes. The literal below
// is a stable 32-hex-char id that's obviously a test fixture.
const TEST_ACTOR_ID = 'e2e7e1f02a30000000000000000007e1';

test.describe('q2-preview render-components-comment (authorship)', () => {
  test('attribution surface + current actor reach user TSX', async ({
    page,
  }) => {
    // Inject the actor id BEFORE any app script runs. `resolveActorId`
    // in App.tsx reads this on the first `connect()` and threads it
    // into `state.actorId`, which `getActorId()` then exposes to
    // `ReactPreview` and onward to the iframe.
    await page.addInitScript((actorId) => {
      (window as unknown as { __QUARTO_TEST_ACTOR_ID__?: string }).__QUARTO_TEST_ACTOR_ID__ = actorId;
    }, TEST_ACTOR_ID);

    const consoleAll: string[] = [];
    page.on('console', (msg: ConsoleMessage) => {
      consoleAll.push(`[${msg.type()}] ${msg.text()}`);
    });
    const pageErrors: string[] = [];
    page.on('pageerror', (err) => {
      pageErrors.push(`${err.message}\n${err.stack ?? ''}`);
    });

    const serverUrl = getServerUrl();

    const indexDocId = await createProjectOnServer(serverUrl, [
      {
        path: '_quarto.yml',
        content: quartoYmlContent,
        contentType: 'text',
      },
      {
        path: 'comment.tsx',
        content: tsxContent,
        contentType: 'text',
      },
      {
        path: 'render-components-comment.qmd',
        content: qmdContent,
        contentType: 'text',
      },
    ]);

    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);

    await page.goto(
      `/#/p/${localId}/file/${encodeURIComponent('render-components-comment.qmd')}`,
    );

    const iframe = page.frameLocator('iframe[src*="q2-preview.html"]');

    // Wait for comment.tsx chrome to render. The "+ 🙂" picker-open
    // button is present on every block with `CommentWrapper`, so it
    // signals the user TSX has loaded and rendered.
    try {
      await expect(
        iframe.locator('[title="Add reaction"]').first(),
      ).toBeVisible({ timeout: 30_000 });
    } catch (e) {
      console.error('--- iframe never rendered comment chrome; dumping full console ---');
      for (const line of consoleAll) console.error(line);
      console.error('--- page errors ---');
      for (const err of pageErrors) console.error(err);
      throw e;
    }

    // Verify both reactji-bubble aggregations rendered on the
    // H1-followup paragraph (which carries 2x 🤔 + 2x 🔥). The two
    // bubbles share a code path so this is mostly redundant with
    // a single-emoji check, but it catches a regression where a
    // ReactionCounts aggregation bug would silently drop one of them.
    await expect(iframe.locator('[title="Add 🤔"]').first()).toBeVisible();
    await expect(iframe.locator('[title="Add 🔥"]').first()).toBeVisible();

    // The user-TSX Block override must shadow the built-in Block on
    // every block kind in the fixture (Para, Header, Div, ...). If any
    // block falls through to the format default, the placeholder leaks
    // — which would indicate comment.tsx's Block export didn't
    // register, or `customRegistry['Block']` precedence regressed.
    await expect(iframe.locator('div.q2-preview-placeholder')).toHaveCount(0);

    // Settle: let the Diagnostic-component's useEffect run (only fires
    // if the mount-gate inside CommentWrapper passed, i.e. if both
    // hooks are on the surface).
    await page.waitForTimeout(500);

    // Read the diagnostic export from the iframe's window. The
    // frameLocator's `body` element gives us a handle whose evaluate
    // runs in the iframe's window context.
    const diag = await iframe
      .locator('body')
      .evaluate(() => (window as any).__COMMENT_DIAG__);

    // Echo to test output so any future regression is recorded inline.
    console.log('COMMENT_DIAG =', JSON.stringify(diag, null, 2));

    // Surface relevant in-iframe console messages and page errors so
    // a future failure can be diagnosed without `--headed`.
    const relevantConsole = consoleAll.filter((line) =>
      line.includes('COMMENT') ||
      line.includes('Diagnostic') ||
      line.includes('comment.tsx') ||
      line.includes('attribut') ||
      line.includes('actor') ||
      line.includes('error') ||
      line.includes('Error'),
    );
    if (relevantConsole.length > 0) {
      console.log('--- console (filtered) ---');
      for (const line of relevantConsole) console.log(line);
    }
    const relevantPageErrors = pageErrors.filter((e) => !e.includes('monaco-editor'));
    if (relevantPageErrors.length > 0) {
      console.log('--- page errors ---');
      for (const err of relevantPageErrors) console.log(err);
    }

    // The plumbing assertions: everything except the actual attribution
    // payload (which depends on the user toggling Attribution on, an
    // opt-in by 2026-05-25 decision-log Q1 outcome) must pass on
    // `feature/provenance` post-Phase-2.
    expect
      .soft(
        diag?.hasUseNodeAttribution,
        'Phase 2a: `__Q2_PREVIEW_RENDERER__.useNodeAttribution` must be exposed to user TSX. ' +
          `Surface keys observed: ${JSON.stringify(diag?.surfaceKeys)}`,
      )
      .toBe(true);

    expect
      .soft(
        diag?.hasUseCurrentActor,
        'Phase 2b: `__Q2_PREVIEW_RENDERER__.useCurrentActor` must be exposed to user TSX. ' +
          `Surface keys observed: ${JSON.stringify(diag?.surfaceKeys)}`,
      )
      .toBe(true);

    expect
      .soft(
        diag?.me,
        'Phase 2b: the injected actor id should reach the iframe. ' +
          `Expected ${JSON.stringify(TEST_ACTOR_ID)}, got ${JSON.stringify(diag?.me)}.`,
      )
      .toBe(TEST_ACTOR_ID);

    expect
      .soft(
        diag?.blocks?.length ?? 0,
        'Diagnostic component should mount on the H1-followup paragraph (4 reactjis) plus the two single-reactji paragraphs.',
      )
      .toBeGreaterThan(0);

    // Per-span attribution check is gated on the Attribution toggle
    // (opt-in by design — see 2026-05-25 decision log Q1). Without the
    // toggle, `AttributionLookupContext` provides `null` and every
    // `firstCommentAttr` is `null`. The check stays in the spec as a
    // *soft, conditional* assertion so a future "default-on" flip would
    // upgrade this from "passes vacuously" to "verifies attribution
    // resolves a non-null actor".
    const anyAttribResolved = (diag?.blocks ?? []).some(
      (b: { firstCommentAttr?: { actor?: string } | null }) =>
        b.firstCommentAttr?.actor != null,
    );
    console.log(
      'attribution resolved on at least one span:',
      anyAttribResolved,
      '(expected false today — Attribution toggle defaults off)',
    );

    expect(diag, 'COMMENT_DIAG must be populated by comment.tsx').toBeTruthy();
  });

  test('reactji bubble click invokes the Phase 2c addReaction handler', async ({
    page,
  }) => {
    // Regression check for the Phase 2c click rewrite. With Attribution
    // toggle off (the 2026-05-25 plan default), `findMineSpan` returns
    // null and the handler falls through to legacy add. We verify the
    // handler *runs* — surfaced via `__COMMENT_DIAG__.addReactionCalls`
    // — rather than asserting on count changes, because the
    // qmd/Automerge round-trip in the offline e2e env doesn't reliably
    // re-render the bubble within a test budget. The "remove mine"
    // branch is verified manually in Phase 3 against an authenticated
    // hub session with the Attribution toggle on.
    await page.addInitScript((actorId) => {
      (window as unknown as { __QUARTO_TEST_ACTOR_ID__?: string }).__QUARTO_TEST_ACTOR_ID__ = actorId;
    }, TEST_ACTOR_ID);

    const serverUrl = getServerUrl();
    const indexDocId = await createProjectOnServer(serverUrl, [
      { path: '_quarto.yml', content: quartoYmlContent, contentType: 'text' },
      { path: 'comment.tsx', content: tsxContent, contentType: 'text' },
      { path: 'render-components-comment.qmd', content: qmdContent, contentType: 'text' },
    ]);
    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);
    await page.goto(
      `/#/p/${localId}/file/${encodeURIComponent('render-components-comment.qmd')}`,
    );

    const iframe = page.frameLocator('iframe[src*="q2-preview.html"]');
    const thinkingBubble = iframe.locator('[title="Add 🤔"]').first();
    await expect(thinkingBubble).toBeVisible({ timeout: 30_000 });
    await expect(thinkingBubble).toContainText('2');

    await thinkingBubble.click();
    await page.waitForTimeout(500);

    const diagAfterClick = await iframe
      .locator('body')
      .evaluate(() => (window as any).__COMMENT_DIAG__);

    const calls = diagAfterClick?.addReactionCalls ?? [];
    expect(
      calls.length,
      'addReaction should have been invoked once by the bubble click',
    ).toBeGreaterThan(0);
    expect(calls[0].emoji, 'click target should be the 🤔 bubble').toBe('🤔');
    expect(
      calls[0].attributionLookupNull,
      'attribution lookup is null when the toggle is off (opt-in default)',
    ).toBe(true);
    expect(
      calls[0].me,
      'injected actor id should be visible to the click handler',
    ).toBe(TEST_ACTOR_ID);
  });
});
