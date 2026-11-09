/**
 * `execute: record-sources`, end to end (RIU-4).
 *
 * Drives the real `q2 preview` binary (embedded SPA + WASM) with `-M execute:{record-sources: ...}` and asserts on the
 * page a reader sees:
 *
 *   1. a `record-sources` directory: outputs from a Positron-format cache file in that directory show under
 *      the cell, with no engine and no kernel (the list has no `engine`).
 *   2. `cache` in `execute: record-sources` on a document nothing has run: the warning box says no source supplied a
 *      record, names each source and why it was passed over, and says how to fix it.
 *
 * Both are the checks that a stale viewer WASM breaks silently (the embedded WASM predates the
 * record-reading code, so the page shows source only with no warning), which no unit or
 * endpoint test can see. Neither needs a language engine, so they run in the default suite.
 *
 * Not covered here: the `--share` guest refusal (needs the iroh relay; asserted at the
 * integration tier and by hand in the RIU-4 plan).
 */

import { createHash } from 'node:crypto';
import { test, expect, type Page } from '@playwright/test';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

const CODE = 'x = [1, 2, 3, 4]\nprint("sum:", sum(x))';

const DOC = `---
title: "Records"
engine: jupyter
---

\`\`\`{python}
${CODE}
\`\`\`
`;

/** Positron's `quarto_content_hash`: the first 16 hex of SHA-1 over the cell's lines. */
function contentHash(text: string): string {
  return createHash('sha1').update(text).digest('hex').slice(0, 16);
}

/** A Positron-format cache file holding one output for `CODE`, found by content hash. */
const CACHE_FILE = JSON.stringify({
  nbformat: 4,
  nbformat_minor: 5,
  metadata: {
    // Names no real file, so the locator falls to its content-hash tier.
    quarto_source: 'file:///nonexistent/records.qmd',
    quarto_cache_version: 1,
    last_updated: 1793809573550,
  },
  cells: [
    {
      cell_type: 'code',
      execution_count: null,
      metadata: {
        quarto_cell_id: '0-' + contentHash(CODE).slice(0, 8) + '-unlabeled',
        quarto_content_hash: contentHash(CODE),
      },
      source: CODE.split('\n').map((l, i, a) => (i < a.length - 1 ? l + '\n' : l)),
      outputs: [
        { output_type: 'stream', name: 'stdout', text: ['sum: FROM-THE-CACHE-FILE\n'] },
      ],
    },
  ],
});

let server: PreviewServerHandle;

test.afterEach(async () => {
  await server?.stop();
});

/** The text of the sandboxed renderer iframe, once it has the heading. */
async function renderedText(page: Page): Promise<string> {
  await page.waitForFunction(
    () => document.querySelector('iframe')?.contentDocument?.querySelector('h1') != null,
    null,
    { timeout: 30_000 },
  );
  return page.evaluate(() => document.querySelector('iframe')?.contentDocument?.body.innerText ?? '');
}

test('a record-sources directory shows its cached outputs under the cell, with no engine', async ({
  page,
}) => {
  server = await startPreviewServer({
    fixtureFiles: [{ path: 'records.qmd', content: DOC }],
    recordsDir: [{ path: 'cache.ipynb', content: CACHE_FILE }],
    targetFile: 'records.qmd',
  });
  await page.goto(server.url);

  await expect
    .poll(async () => (await renderedText(page)).includes('sum: FROM-THE-CACHE-FILE'), {
      timeout: 30_000,
    })
    .toBe(true);
  // A source supplied, so the server raises no "no record" warning.
  await expect(page.getByText('No execution record for this document')).toHaveCount(0);
});

test('record-sources `cache` on a document with no record explains why on the page', async ({ page }) => {
  server = await startPreviewServer({
    fixtureFiles: [{ path: 'records.qmd', content: DOC }],
    records: ['cache'],
    targetFile: 'records.qmd',
  });
  await page.goto(server.url);

  // The warning box starts collapsed as a "Warning" button; open it.
  const warning = page.getByRole('button', { name: /warning/i });
  await expect(warning).toBeVisible({ timeout: 30_000 });
  await warning.click();

  await expect(page.getByText('No execution record for this document')).toBeVisible();
  // The per-source reasons and the fix are on the page, not only in the endpoint.
  await expect(page.getByText(/cache: nothing for this document/)).toBeVisible();
  await expect(page.getByText(/engine: not available/)).toBeVisible();
  await expect(page.getByText(/Add `engine` to `execute: record-sources`/)).toBeVisible();
  // And no output was invented: the cell is source only.
  expect(await renderedText(page)).not.toContain('FROM-THE-CACHE-FILE');
});
