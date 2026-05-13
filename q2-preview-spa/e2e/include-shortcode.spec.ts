/**
 * Cross-doc include shortcode propagation (bd-pf63, Phase B.3).
 *
 * Pins that editing an *included* file on disk causes the
 * *includer*'s preview to re-render with the new content. The
 * Phase A pipeline (watcher → samod sync → SPA `onFileContent` →
 * VFS update → `contentTick` bump → render useEffect) is meant to
 * cover this for any file the active page transitively depends on;
 * B.3 is the empirical proof.
 *
 * Fixture shape:
 *
 *   index.qmd      # Includer page
 *                  {{< include x.qmd >}}
 *
 *   x.qmd          ## Section from included
 *                  SENTINEL-INITIAL
 *
 * The H1 "Includer page" only appears if `index.qmd` is the active
 * page (because `x.qmd` has no H1). That doubles as a sanity check:
 * if the SPA ever picked `x.qmd` as the active page instead, the
 * initial render assertion would fail loudly rather than silently
 * trivialising the cross-doc check.
 *
 * Per plan §B.3: if this spec fails, do NOT patch speculatively —
 * file a bd-issue describing which leg of the pipeline didn't fire
 * and hand back. The whole point of this verification is to surface
 * gaps in the existing machinery.
 */

import { test, expect, type Page } from '@playwright/test';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

const INDEX_QMD = `# Includer page

{{< include x.qmd >}}
`;

const INCLUDED_INITIAL = `## Section from included

SENTINEL-INITIAL
`;

const INCLUDED_EDITED = `## Section from included

SENTINEL-EDITED
`;

/**
 * Same shape as `basic-preview.spec.ts:waitForInnerHeading` — wait
 * for the outer iframe's contentDocument to expose an H1 whose
 * textContent matches. 30 s gives the SPA's WASM boot enough head-
 * room on cold CI workers.
 */
async function waitForInnerH1(page: Page, text: string) {
  await page.waitForFunction(
    (expected) => {
      const outer = document.querySelector('iframe');
      const innerDoc = outer?.contentDocument;
      const h1 = innerDoc?.querySelector('h1');
      return h1 != null && h1.textContent === expected;
    },
    text,
    { timeout: 30_000 },
  );
}

/** Read all paragraph textContents from the inner (renderer) iframe. */
async function innerParagraphs(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const outer = document.querySelector('iframe');
    const innerDoc = outer?.contentDocument;
    return Array.from(innerDoc?.querySelectorAll('p') ?? []).map(
      (p) => p.textContent ?? '',
    );
  });
}

let server: PreviewServerHandle;

test.beforeEach(async () => {
  server = await startPreviewServer({
    fixtureFiles: [
      { path: 'index.qmd', content: INDEX_QMD },
      { path: 'x.qmd', content: INCLUDED_INITIAL },
    ],
  });
});

test.afterEach(async () => {
  await server?.stop();
});

test('include shortcode expands x.qmd into index.qmd on initial render', async ({ page }) => {
  await page.goto(server.url);
  await waitForInnerH1(page, 'Includer page');

  // The H2 from x.qmd should appear in the includer's rendered
  // output — this is the load-bearing assertion that the include
  // shortcode actually expanded against the VFS view of x.qmd.
  const innerHeadings = await page.evaluate(() => {
    const outer = document.querySelector('iframe');
    const innerDoc = outer?.contentDocument;
    return {
      h1: innerDoc?.querySelector('h1')?.textContent ?? null,
      h2: innerDoc?.querySelector('h2')?.textContent ?? null,
    };
  });
  expect(innerHeadings.h1).toBe('Includer page');
  expect(innerHeadings.h2).toBe('Section from included');

  const paragraphs = await innerParagraphs(page);
  expect(paragraphs.some((p) => p.includes('SENTINEL-INITIAL'))).toBe(true);
  // Defensive: the un-edited sentinel should NOT contain the
  // post-edit marker, otherwise the edit-phase assertion below
  // would trivially pass on stale state.
  expect(paragraphs.some((p) => p.includes('SENTINEL-EDITED'))).toBe(false);
});

test('editing x.qmd on disk re-renders index.qmd within 5s', async ({ page }) => {
  await page.goto(server.url);
  await waitForInnerH1(page, 'Includer page');

  // Establish that the initial sentinel is present — guards against
  // the watcher firing for an unrelated reason during boot and
  // producing a confusing pass.
  const initialParagraphs = await innerParagraphs(page);
  expect(initialParagraphs.some((p) => p.includes('SENTINEL-INITIAL'))).toBe(true);

  // Edit the *includee* on disk. The watcher inside quarto-hub
  // (PreviewBroad filter, set by B.1) picks up the .qmd change,
  // syncs new bytes into samod, the SPA's onFileContent fires for
  // x.qmd (after vfsAddFile has updated VFS), and contentTick bumps
  // — which re-fires the render useEffect for the *active* page
  // (index.qmd). Include-expansion reads the updated x.qmd bytes
  // from VFS, so the includer's rendered output should reflect the
  // edit.
  await writeFile(path.join(server.projectDir, 'x.qmd'), INCLUDED_EDITED);

  await page.waitForFunction(
    () => {
      const outer = document.querySelector('iframe');
      const innerDoc = outer?.contentDocument;
      const paragraphs = Array.from(innerDoc?.querySelectorAll('p') ?? []);
      return paragraphs.some((p) => p.textContent?.includes('SENTINEL-EDITED'));
    },
    null,
    // Plan budget is 2 s; matches basic-preview.spec.ts's 5 s CI
    // ceiling. notify-rs debounce (500 ms in HubConfig defaults) +
    // samod sync round-trip + WASM render comfortably fit.
    { timeout: 5_000 },
  );

  // After the edit propagates, INITIAL should be gone and EDITED
  // should be the only sentinel — verifies the includer was actually
  // re-rendered against the new x.qmd bytes, not just appended to.
  const after = await innerParagraphs(page);
  expect(after.some((p) => p.includes('SENTINEL-EDITED'))).toBe(true);
  expect(after.some((p) => p.includes('SENTINEL-INITIAL'))).toBe(false);

  // The includer's own H1 must still be present — sanity check that
  // a re-render replaced the body of the includer, not the includer
  // itself.
  const h1 = await page.evaluate(() => {
    const outer = document.querySelector('iframe');
    return outer?.contentDocument?.querySelector('h1')?.textContent ?? null;
  });
  expect(h1).toBe('Includer page');
});
