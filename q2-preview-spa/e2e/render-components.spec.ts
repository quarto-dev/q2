/**
 * `q2 preview` render-components end-to-end (GH #402 / bd-ue80chl0).
 *
 * Before this feature, the CLI preview silently dropped
 * `render-components:` front-matter entries — the parent half (meta
 * walk → transpile → LOAD_CUSTOM_COMPONENTS) existed only in
 * hub-client. This spec drives the real `q2 preview` binary against
 * the same fixture the hub-client smoke harness uses
 * (`crates/quarto/tests/smoke-all/q2-preview/with-render-components/`)
 * and pins:
 *
 *   1. Both user overrides fire (`p.my-para` from the Pandoc-tag
 *      override, `div.my-callout` from the CustomNode override) and
 *      the built-in Callout markup does NOT render — the fixture's own
 *      ensureHtmlElements contract, now holding under the CLI too.
 *   2. Editing the `.tsx` on disk live-repaints the preview with the
 *      re-transpiled component (tsxTick → re-transpile →
 *      LOAD_CUSTOM_COMPONENTS → iframe repaint of the cached AST).
 */

import { test, expect, type Page } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

const FIXTURE_DIR = path.resolve(
  import.meta.dirname,
  '../../crates/quarto/tests/smoke-all/q2-preview/with-render-components',
);

const qmdContent = readFileSync(path.join(FIXTURE_DIR, 'index.qmd'), 'utf-8');
const tsxContent = readFileSync(path.join(FIXTURE_DIR, 'overrides.tsx'), 'utf-8');
const quartoYml = readFileSync(path.join(FIXTURE_DIR, '_quarto.yml'), 'utf-8');

/**
 * Query the inner (sandboxed q2-preview) iframe document for the
 * override / built-in markers. Same inner-iframe access pattern as
 * `basic-preview.spec.ts`.
 */
async function readMarkers(page: Page) {
  return page.evaluate(() => {
    const innerDoc = document.querySelector('iframe')?.contentDocument;
    return {
      myPara: innerDoc?.querySelectorAll('p.my-para').length ?? 0,
      myParaV2: innerDoc?.querySelectorAll('p.my-para-v2').length ?? 0,
      myCallout: innerDoc?.querySelectorAll('div.my-callout').length ?? 0,
      builtinCallout: innerDoc?.querySelectorAll('div.callout').length ?? 0,
    };
  });
}

let server: PreviewServerHandle;

test.afterEach(async () => {
  await server?.stop();
});

/** Wait until the given selector appears in the inner iframe document. */
async function waitForInnerSelector(page: Page, selector: string) {
  await page.waitForFunction(
    (sel) => {
      const innerDoc = document.querySelector('iframe')?.contentDocument;
      return (innerDoc?.querySelectorAll(sel).length ?? 0) > 0;
    },
    selector,
    { timeout: 30_000 },
  );
}

test.beforeEach(async ({}, testInfo) => {
  if (testInfo.title.includes('[single-file]')) {
    // Single-file mode: `q2 preview index.qmd` — no `_quarto.yml`, the
    // deck's directory becomes the synthetic project root, and the
    // `.tsx` reaches the VFS only through `resolve_single_file_deps`'s
    // render-components closure (Phase 3).
    server = await startPreviewServer({
      fixtureFiles: [
        { path: 'index.qmd', content: qmdContent },
        { path: 'overrides.tsx', content: tsxContent },
      ],
      targetFile: 'index.qmd',
    });
  } else {
    server = await startPreviewServer({
      fixtureFiles: [
        { path: '_quarto.yml', content: quartoYml },
        { path: 'index.qmd', content: qmdContent },
        { path: 'overrides.tsx', content: tsxContent },
      ],
    });
  }
});

test('user render-components overrides shadow the built-ins', async ({ page }) => {
  await page.goto(server.url);

  await page.waitForFunction(
    () => {
      const innerDoc = document.querySelector('iframe')?.contentDocument;
      return (
        (innerDoc?.querySelectorAll('p.my-para').length ?? 0) > 0 &&
        (innerDoc?.querySelectorAll('div.my-callout').length ?? 0) > 0
      );
    },
    undefined,
    { timeout: 30_000 },
  );

  const markers = await readMarkers(page);
  expect(markers.myPara).toBeGreaterThan(0);
  expect(markers.myCallout).toBeGreaterThan(0);
  // The user Callout override replaces the built-in wholesale — the
  // built-in's `div.callout` chrome must not appear anywhere.
  expect(markers.builtinCallout).toBe(0);
});

test('editing the .tsx on disk live-repaints with the new component', async ({ page }) => {
  await page.goto(server.url);
  await page.waitForFunction(
    () => {
      const innerDoc = document.querySelector('iframe')?.contentDocument;
      return (innerDoc?.querySelectorAll('p.my-para').length ?? 0) > 0;
    },
    undefined,
    { timeout: 30_000 },
  );

  // Rename the Para override's class on disk. The watcher accepts
  // `.tsx` (WatchFilter::PreviewBroad), the change syncs into the SPA,
  // tsxTick re-transpiles, and the iframe repaints the *cached* AST —
  // no .qmd edit happens in this test.
  await writeFile(
    path.join(server.projectDir, 'overrides.tsx'),
    tsxContent.replace(`className: 'my-para'`, `className: 'my-para-v2'`),
  );

  await page.waitForFunction(
    () => {
      const innerDoc = document.querySelector('iframe')?.contentDocument;
      return (innerDoc?.querySelectorAll('p.my-para-v2').length ?? 0) > 0;
    },
    undefined,
    { timeout: 30_000 },
  );

  const markers = await readMarkers(page);
  expect(markers.myParaV2).toBeGreaterThan(0);
  expect(markers.myPara).toBe(0);
});

test('[single-file] overrides load and a .tsx edit live-repaints', async ({ page }) => {
  await page.goto(server.url);

  // Phase 3 core claim: in single-file mode the `.tsx` reaches the VFS
  // via the render-components closure, so both overrides fire.
  await waitForInnerSelector(page, 'p.my-para');
  await waitForInnerSelector(page, 'div.my-callout');
  expect((await readMarkers(page)).builtinCallout).toBe(0);

  // Watcher-enrollment claim: the closure also enrolls the `.tsx` in
  // the (non-recursive, closure-scoped) single-file watcher, so a disk
  // edit re-syncs, re-transpiles, and repaints.
  await writeFile(
    path.join(server.projectDir, 'overrides.tsx'),
    tsxContent.replace(`className: 'my-para'`, `className: 'my-para-v2'`),
  );
  await waitForInnerSelector(page, 'p.my-para-v2');
  expect((await readMarkers(page)).myPara).toBe(0);
});
