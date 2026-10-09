/**
 * Live PDF preview of `format: typst` pages in the viewer SPA (Plan 11).
 *
 * Spawns the real `q2 preview` binary: the server compiles with native typst
 * (`POST /api/preview/pdf`) and the SPA shows the bytes in the bundled pdf.js. Skips without a
 * `typst` or `pandoc` on PATH, and when the embedded SPA is the placeholder tree (no
 * `/pdfjs/` to load). No non-vendored typst package is used: CI has no network for `@preview`.
 */

import { test, expect, type FrameLocator, type Page } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { startPreviewServer, type FixtureFile, type PreviewServerHandle } from './helpers/previewServer';

function onPath(tool: string): boolean {
  try {
    execFileSync(tool, ['--version'], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

const doc = (heading: string, body = 'Some body text.') =>
  `---\ntitle: Fixture\nformat: typst\n---\n\n# ${heading}\n\n${body}\n`;

let server: PreviewServerHandle | undefined;

// A cold compile plus pdf.js startup outlasts the 30 s default.
test.setTimeout(90_000);

test.beforeEach(() => {
  test.skip(!onPath('typst'), 'native typst is not on PATH');
  test.skip(!onPath('pandoc'), 'native pandoc is not on PATH');
});

test.afterEach(async () => {
  await server?.stop();
  server = undefined;
});

/** Start a server, skipping the test when the embedded SPA is the placeholder (no pdf.js tree). */
async function start(fixtureFiles: FixtureFile[]): Promise<PreviewServerHandle> {
  server = await startPreviewServer({ fixtureFiles });
  const probe = await fetch(`${server.url}pdfjs/web/viewer.html`);
  test.skip(!probe.ok, 'the embedded SPA is the placeholder: no pdf.js tree to load');
  return server;
}

const pdfFrame = (page: Page): FrameLocator => page.frameLocator('iframe[title="PDF preview"]');
const textLayer = (page: Page) => pdfFrame(page).locator('.textLayer');

/** All pages' text via pdf.js, for documents whose interesting page is not the first (text layers exist only for visible pages). */
async function allPdfText(page: Page): Promise<string> {
  const frame = page.frames().find((f) => f.url().includes('/pdfjs/web/viewer.html'));
  if (!frame) return '';
  return frame.evaluate(async () => {
    const app = (window as unknown as { PDFViewerApplication?: { pdfDocument?: any } }).PDFViewerApplication;
    const pdf = app?.pdfDocument;
    if (!pdf) return '';
    let out = '';
    for (let i = 1; i <= pdf.numPages; i++) {
      const content = await (await pdf.getPage(i)).getTextContent();
      out += content.items.map((it: { str?: string }) => it.str ?? '').join(' ') + '\n';
    }
    return out.replace(/\s+/g, ' ');
  });
}

test('a typst page shows a PDF with its heading in the text layer', async ({ page }) => {
  const s = await start([{ path: 'index.qmd', content: doc('Alpha heading') }]);
  await page.goto(s.url);
  await expect(textLayer(page)).toContainText('Alpha heading', { timeout: 60_000 });
  await expect(page.locator('iframe[src*="q2-preview.html"]')).toHaveCount(0);
});

test('an edit recompiles the PDF', async ({ page }) => {
  const s = await start([{ path: 'index.qmd', content: doc('Alpha heading') }]);
  await page.goto(s.url);
  await expect(textLayer(page)).toContainText('Alpha heading', { timeout: 60_000 });
  await writeFile(path.join(s.projectDir, 'index.qmd'), doc('Beta heading'));
  await expect(textLayer(page)).toContainText('Beta heading', { timeout: 60_000 });
});

test('a broken edit shows the banner and keeps the last PDF', async ({ page }) => {
  const s = await start([{ path: 'index.qmd', content: doc('Alpha heading') }]);
  await page.goto(s.url);
  await expect(textLayer(page)).toContainText('Alpha heading', { timeout: 60_000 });
  await writeFile(
    path.join(s.projectDir, 'index.qmd'),
    doc('Alpha heading', '```{=typst}\n#this-function-does-not-exist(\n```'),
  );
  await expect(page.getByRole('alert')).toBeVisible({ timeout: 60_000 });
  await expect(textLayer(page)).toContainText('Alpha heading');
});

test('an output-ext: typ page keeps the "no live preview" overlay, not a PDF', async ({ page }) => {
  const s = await start([
    {
      path: 'index.qmd',
      content: '---\ntitle: T\nformat:\n  typst:\n    output-ext: typ\n---\n\n# Source output\n',
    },
  ]);
  await page.goto(s.url);
  await expect(page.getByText(/no live preview/i)).toBeVisible({ timeout: 60_000 });
  await expect(page.locator('iframe[title="PDF preview"]')).toHaveCount(0);
});

test('a project document inheriting format: typst shows a PDF', async ({ page }) => {
  const s = await start([
    { path: '_quarto.yml', content: 'project:\n  type: default\nformat: typst\n' },
    { path: 'index.qmd', content: '---\ntitle: Inherit\n---\n\n# Inherited heading\n' },
  ]);
  await page.goto(s.url);
  await expect(textLayer(page)).toContainText('Inherited heading', { timeout: 60_000 });
});

test('a document using a vendored typst package (.column-margin) compiles', async ({ page }) => {
  const s = await start([
    {
      path: 'index.qmd',
      content: doc('Margin heading', '::: {.column-margin}\nA margin note.\n:::\n\nMain text.'),
    },
  ]);
  await page.goto(s.url);
  await expect(textLayer(page)).toContainText('Margin heading', { timeout: 60_000 });
  await expect(textLayer(page)).toContainText('A margin note.');
});

test('a book chapter previews alone (chapter scope)', async ({ page }) => {
  const s = await start([
    {
      path: '_quarto.yml',
      content: 'project:\n  type: book\nbook:\n  title: The Book\n  author: Test Author\n  chapters:\n    - index.qmd\n    - two.qmd\nformat: typst\n',
    },
    { path: 'index.qmd', content: '# First chapter heading\n\nOne.\n' },
    { path: 'two.qmd', content: '# Second chapter heading\n\nTwo.\n' },
  ]);
  const url = new URL(s.url);
  url.searchParams.set('page', 'two.qmd');
  await page.goto(url.toString());
  await expect.poll(() => allPdfText(page), { timeout: 60_000 }).toContain('Second chapter heading');
  expect(await allPdfText(page)).not.toContain('First chapter heading');
});
