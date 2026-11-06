/**
 * A page's shared engine record reaches a chapter-alone download (bd-zykj3dmx, EXR-15).
 *
 * `q2 provide-hub` records a document's execution on the hub and names it in the index doc's
 * `captures` sidecar. The browser has no engines, so a "Download as" of one page shows
 * executed output only when the download fetches that page's capture and the renderer splices
 * it. Before bd-zykj3dmx only a whole-book export did; a chapter-alone download (the "This
 * chapter only" entry, or an ordinary page) rendered the cells as unexecuted source.
 *
 * The capture here is a *record* (EXR-13): the checked-in `records/basic.json` fixture with its
 * one output replaced by a marker. Outputs are not part of any hash, so the python cell still
 * matches its source by unit hash. The test writes the doc and the sidecar entry the way the
 * provider does (`createProjectOnServer`'s `captures` argument), so the whole browser chain runs
 * for real: sidecar -> `fetchChapterCaptures` -> `captureGzJson` -> WASM reader -> splice ->
 * pandoc.wasm -> a downloaded file.
 *
 * Run via:
 *   cd hub-client && npx playwright test e2e/chapter-capture-download.spec.ts --project=chromium --workers=1
 */
import { test, expect, type Page } from '@playwright/test';
import { gzipSync, unzipSync, strFromU8 } from 'fflate';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import type {} from './helpers/testHooks';
import {
  bootstrapProjectSet,
  createProjectOnServer,
  seedProjectInBrowser,
  getServerUrl,
  type EnginePayload,
  type ProjectFile,
} from './helpers/projectFactory';

const FIXTURE = resolve(import.meta.dirname, '../../crates/quarto-core/tests/fixtures/records/basic.json');
const MARKER = 'E2E_RECORDED_OUTPUT_7f3a';

/** The one python cell the fixture records, in a page with a title and some prose. */
function chapter(title: string): string {
  return ['---', `title: ${title}`, '---', '', `# ${title}`, '', '```{python}', 'print(1)', '```', '', 'After the cell.', ''].join('\n');
}

/** The fixture as a gzipped record whose one output is {@link MARKER}. */
async function recordPayload(): Promise<EnginePayload> {
  const record = JSON.parse(await readFile(FIXTURE, 'utf8'));
  record.cells[0].outputs[0].text = [`${MARKER}\n`];
  return { kind: 'record', gzipped: gzipSync(new TextEncoder().encode(JSON.stringify(record))) };
}

async function openProject(
  page: Page,
  files: ProjectFile[],
  path: string,
  captures?: Record<string, EnginePayload>,
): Promise<void> {
  const serverUrl = getServerUrl();
  const docId = await createProjectOnServer(serverUrl, files, captures);
  await bootstrapProjectSet(page);
  const localId = await seedProjectInBrowser(page, docId, serverUrl);
  await page.goto(`/#/p/${localId}/file/${path}`);
}

/** Every text entry of the downloaded zip (docx or epub), concatenated. */
async function zipText(download: { path(): Promise<string | null> }): Promise<string> {
  const file = await download.path();
  expect(file).not.toBeNull();
  const zip = unzipSync(new Uint8Array(await readFile(file!)));
  return Object.entries(zip)
    .filter(([name]) => /\.(xml|xhtml|html|opf|ncx)$/.test(name))
    .map(([, bytes]) => strFromU8(bytes))
    .join('\n');
}

const PAGE_FILES: ProjectFile[] = [{ path: 'page.qmd', content: chapter('Recorded page'), contentType: 'text' }];

const BOOK_FILES: ProjectFile[] = [
  { path: '_quarto.yml', content: 'project:\n  type: book\nbook:\n  title: Small Book\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n', contentType: 'text' },
  { path: 'index.qmd', content: '# Preface\n\nHello preface.\n', contentType: 'text' },
  { path: 'one.qmd', content: chapter('Chapter One'), contentType: 'text' },
  { path: 'two.qmd', content: chapter('Chapter Two'), contentType: 'text' },
];

test.describe('Chapter-alone download carries the page capture', () => {
  // The first download fetches and compiles pandoc.wasm.
  test.setTimeout(180_000);

  async function downloadWord(page: Page) {
    await page.getByRole('button', { name: 'Download as' }).click({ timeout: 30000 });
    const downloadPromise = page.waitForEvent('download', { timeout: 120000 });
    await page.getByRole('menuitem', { name: 'Word' }).click();
    return downloadPromise;
  }

  test('an ordinary page with a record downloads the recorded output', async ({ page }) => {
    await openProject(page, PAGE_FILES, 'page.qmd', { 'page.qmd': await recordPayload() });
    const download = await downloadWord(page);
    expect(download.suggestedFilename()).toBe('page.docx');
    const text = await zipText(download);
    expect(text).toContain(MARKER);
    expect(text).toContain('After the cell.');
  });

  test('the same page without a capture downloads the cell as source (control)', async ({ page }) => {
    await openProject(page, PAGE_FILES, 'page.qmd');
    const download = await downloadWord(page);
    const text = await zipText(download);
    expect(text).toContain('print(1)');
    expect(text).not.toContain(MARKER);
  });

  test('"This chapter only" downloads that chapter with its recorded output and no other chapter', async ({ page }) => {
    await openProject(page, BOOK_FILES, 'one.qmd', { 'one.qmd': await recordPayload() });
    await page.getByRole('button', { name: 'Download as' }).click({ timeout: 30000 });
    const downloadPromise = page.waitForEvent('download', { timeout: 120000 });
    await page.getByRole('menuitem', { name: /This chapter only/ }).filter({ hasText: 'EPUB' }).click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toBe('one.epub');
    const text = await zipText(download);
    expect(text).toContain(MARKER);
    expect(text).not.toContain('Chapter Two');
  });
});
