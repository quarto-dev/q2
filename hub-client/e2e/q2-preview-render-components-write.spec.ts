/**
 * E2E repro for "Incremental write failed: undefined" on q2-preview.
 *
 * Sister to `q2-debug-render-components.spec.ts`, but the click here
 * triggers `setLocalAst` (not local React state). That threads through
 * the renderer dispatch into `ReactPreview.handleSetAst` →
 * `incrementalWriteQmd` (`ts-packages/preview-runtime/src/wasmRenderer.ts`),
 * which is the path the user hit while clicking a reactji button in the
 * `render-components` demo.
 *
 * The fixture's `write-reaction.tsx` mirrors the addReaction code in
 * `~/docs/demo-playground/gordon/render-components/comment.tsx`: append a
 * fresh `Span.quarto-edit-comment` to the clicked Para's inline children
 * and `setLocalAst(newBlock)`. The dispatch wraps that into a full AST
 * (one block replaced) and feeds it as the new-AST to the WASM bridge.
 *
 * Expected after the bug is fixed: the write succeeds and no "Incremental
 * write failed" console error fires.
 *
 * Current behaviour (the bug we're chasing): the bridge returns
 * `{success: true, qmd: '', warnings: [Q-3-43]}` — empty document with a
 * "Generated content edit dropped" warning. The wasmRenderer.ts:758
 * throw site (instrumented to distinguish this empty-qmd path) logs
 * `incrementalWriteQmd failed; raw response: ...` and throws.
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
  '../../crates/quarto/tests/smoke-all/q2-preview/render-components-write',
);

const qmdContent = readFileSync(resolve(FIXTURE_DIR, 'index.qmd'), 'utf-8');
const commentTsxContent = readFileSync(
  resolve(FIXTURE_DIR, 'comment.tsx'),
  'utf-8',
);
const kanbanTsxContent = readFileSync(
  resolve(FIXTURE_DIR, 'kanban.tsx'),
  'utf-8',
);
const dragTsxContent = readFileSync(
  resolve(FIXTURE_DIR, 'drag.tsx'),
  'utf-8',
);
const quartoYmlContent = readFileSync(
  resolve(FIXTURE_DIR, '_quarto.yml'),
  'utf-8',
);

test.describe('q2-preview render-components write', () => {
  test('clicking +react triggers setLocalAst → incremental_write_qmd without empty-qmd error', async ({
    page,
  }) => {
    const serverUrl = getServerUrl();

    // Collect every console.error from the page (and its iframes). The
    // instrumentation in `wasmRenderer.ts:758` emits
    //   `incrementalWriteQmd failed; raw response: { ... }`
    // when the WASM bridge returns Ok with an empty qmd string. We assert
    // no such message lands during the click → write round-trip.
    const consoleErrors: string[] = [];
    const consoleAll: string[] = [];
    page.on('console', (msg: ConsoleMessage) => {
      const loc = msg.location();
      const tag = `[${msg.type()}] ${msg.text()} @ ${loc.url}:${loc.lineNumber}`;
      consoleAll.push(tag);
      if (msg.type() === 'error') {
        consoleErrors.push(msg.text());
      }
    });
    // Surface page errors too so a thrown JS error doesn't look like a
    // silent pass.
    const pageErrors: string[] = [];
    page.on('pageerror', (err) => {
      pageErrors.push(`${err.message}\n${err.stack ?? ''}`);
    });

    const indexDocId = await createProjectOnServer(serverUrl, [
      {
        path: '_quarto.yml',
        content: quartoYmlContent,
        contentType: 'text',
      },
      {
        path: 'comment.tsx',
        content: commentTsxContent,
        contentType: 'text',
      },
      {
        path: 'kanban.tsx',
        content: kanbanTsxContent,
        contentType: 'text',
      },
      {
        path: 'drag.tsx',
        content: dragTsxContent,
        contentType: 'text',
      },
      {
        path: 'index.qmd',
        content: qmdContent,
        contentType: 'text',
      },
    ]);

    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);

    await page.goto(
      `/#/p/${localId}/file/${encodeURIComponent('index.qmd')}`,
    );

    // The q2-preview iframe is `q2-preview.html`, distinct from the
    // q2-debug iframe used by the sister spec. The user's CommentWrapper
    // renders a "+ 🙂" button (title="Add reaction") next to every Para;
    // clicking it opens an emoji picker, clicking an emoji calls
    // addReaction → setLocalAst.
    const iframe = page.frameLocator('iframe[src*="q2-preview.html"]');

    // Wait for the iframe to render the first paragraph's CommentWrapper
    // chrome — the "+ 🙂" emoji-picker open button.
    const openPicker = iframe.locator('[title="Add reaction"]').first();
    try {
      await expect(openPicker).toBeVisible({ timeout: 30_000 });
    } catch (e) {
      console.error('--- console messages so far ---');
      for (const line of consoleAll) console.error(line);
      console.error('--- page errors ---');
      for (const err of pageErrors) console.error(err);
      throw e;
    }

    // Open the picker, then click the 😂 emoji. Picker emoji spans
    // carry no test id — locate by text. There's a 😂 in
    // CommentWrapper's `commonEmojis` list (`'👍', '❤️', '😂', ...`).
    await openPicker.click();
    await iframe.locator('text="😂"').first().click();

    // Give the WASM call time to run and emit its console.error if it
    // hits the failure path.
    await page.waitForTimeout(1500);

    const writeFailures = consoleErrors.filter((line) =>
      line.includes('incrementalWriteQmd failed'),
    );

    if (writeFailures.length > 0) {
      console.error('--- Full console log on failure ---');
      for (const line of consoleAll) console.error(line);
    }

    expect(
      writeFailures,
      'Incremental write should not fail when appending a reaction to the first paragraph. ' +
        'Raw console errors:\n' +
        consoleErrors.join('\n'),
    ).toEqual([]);

    // Filter out unrelated Monaco loader internal errors (Monaco runs
    // inside the markup-view panel; its load can throw without
    // affecting the preview).
    const relevantPageErrors = pageErrors.filter(
      (e) => !e.includes('monaco-editor'),
    );
    expect(relevantPageErrors, 'Page should not throw').toEqual([]);
  });
});
