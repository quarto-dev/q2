/**
 * hub-parity Plan 13: book scope in `q2 preview --ui editor`.
 *
 * On a chapter of a book project the Download as menu offers "Download book as X" and "This chapter only" for
 * typst source, PDF and EPUB (the same code as hub-client), and the native route renders either the whole book or
 * the chapter alone. Covered here:
 *   - the book's typst source holds every chapter, and the chapter-only `.typ` holds just the active one;
 *   - the book PDF and the chapter-only PDF are named for the book and the chapter respectively;
 *   - a page that is not one of the book's chapters downloads standalone, and a stale "book" request for it comes
 *     back as that page alone with the `Q-5-48` notice.
 *
 * Needs a `q2` built with the real embed (`cargo xtask build-hub-client-embed`, then `cargo build --bin q2`),
 * and pandoc and typst on PATH. The PDF case also needs the network (the book template fetches
 * `@preview/orange-book`). Each missing prerequisite skips with its reason.
 */

import { test, expect, type Page } from '@playwright/test';
import { mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { isEmbedBuilt, startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

function onPath(cmd: string): boolean {
  try {
    execFileSync(cmd, ['--version'], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

async function packagesReachable(): Promise<boolean> {
  try {
    const res = await fetch('https://packages.typst.org/preview/index.json', { method: 'HEAD', signal: AbortSignal.timeout(5_000) });
    return res.ok;
  } catch {
    return false;
  }
}

// `book.author` is required by the book template; without it typst fails with "expected content, found array".
const BOOK_FILES = [
  {
    path: '_quarto.yml',
    content: 'project:\n  type: book\nbook:\n  title: Small Book\n  author: Test Author\n  chapters:\n    - index.qmd\n    - two.qmd\n    - three.qmd\nformat: typst\n',
  },
  { path: 'index.qmd', content: '# First chapter heading\n\nToken ALPHA.\n' },
  { path: 'two.qmd', content: '# Second chapter heading\n\nToken BETA.\n' },
  { path: 'three.qmd', content: '# Third chapter heading\n\nToken GAMMA.\n' },
  // Not in `book.chapters`.
  { path: 'other.qmd', content: '---\ntitle: Other\nformat: typst\n---\n\n# Standalone heading\n\nToken DELTA.\n' },
];

let server: PreviewServerHandle | undefined;
test.afterEach(async () => {
  await server?.stop();
  server = undefined;
});

/** Start the editor UI over the book fixture; skips when a prerequisite or the real embed is missing. */
async function startBook(): Promise<PreviewServerHandle> {
  test.skip(!onPath('pandoc'), 'pandoc is not on PATH');
  test.skip(!onPath('typst'), 'typst is not on PATH');
  server = await startPreviewServer({ fixtureFiles: BOOK_FILES, ui: 'editor' });
  test.skip(!(await isEmbedBuilt(server)), 'q2 was built without the hub-client embed (placeholder)');
  return server;
}

/** Open the file in the sidebar tree and wait for the Download as button (the editor is ready). */
async function openFile(page: Page, name: string): Promise<void> {
  await page.getByRole('treeitem', { name: new RegExp(`^${name.replace('.', '\\.')}`) }).click();
  await expect(page.getByRole('button', { name: 'Download as' })).toBeVisible({ timeout: 60_000 });
}

async function download(page: Page, label: RegExp): Promise<{ file: string; name: string }> {
  await page.getByRole('button', { name: 'Download as' }).click();
  const [dl] = await Promise.all([page.waitForEvent('download', { timeout: 120_000 }), page.getByRole('menuitem', { name: label }).click()]);
  const dir = await mkdtemp(path.join(tmpdir(), 'q2-embed-book-'));
  const file = path.join(dir, dl.suggestedFilename());
  await dl.saveAs(file);
  return { file, name: dl.suggestedFilename() };
}

test.describe('q2 preview --ui editor: book scope', () => {
  test('the menu offers the book entries on a chapter; the typst source is the book or the chapter alone', async ({ page }) => {
    const s = await startBook();
    await page.goto(s.url);
    await openFile(page, 'two.qmd');

    await page.getByRole('button', { name: 'Download as' }).click();
    await expect(page.getByRole('menuitem', { name: /Download book as .*\.typ\b/i })).toBeVisible();
    await expect(page.getByRole('menuitem', { name: /This chapter only.*\.typ\b/i })).toBeVisible();
    await page.keyboard.press('Escape');

    const book = await download(page, /Download book as .*\.typ\b/i);
    expect(book.name).toBe('Small-Book.typ');
    const bookSource = await readFile(book.file, 'utf8');
    for (const token of ['ALPHA', 'BETA', 'GAMMA']) expect(bookSource).toContain(token);
    await expect(page.getByTestId('download-status').getByText('Book downloaded (3 chapters)')).toBeVisible();

    const chapter = await download(page, /This chapter only.*\.typ\b/i);
    expect(chapter.name).toBe('two.typ');
    const chapterSource = await readFile(chapter.file, 'utf8');
    expect(chapterSource).toContain('BETA');
    expect(chapterSource).not.toContain('ALPHA');
    expect(chapterSource).not.toContain('GAMMA');
  });

  test('the book PDF and the chapter-only PDF are real PDFs named for the book and the chapter', async ({ page }) => {
    test.setTimeout(240_000);
    test.skip(!(await packagesReachable()), 'packages.typst.org is not reachable (the book template needs @preview/orange-book)');
    const s = await startBook();
    await page.goto(s.url);
    await openFile(page, 'two.qmd');

    const book = await download(page, /Download book as .*\.pdf/i);
    expect(book.name).toBe('Small-Book.pdf');
    expect((await readFile(book.file)).subarray(0, 5).toString()).toBe('%PDF-');

    const chapter = await download(page, /This chapter only.*\.pdf/i);
    expect(chapter.name).toBe('two.pdf');
    expect((await readFile(chapter.file)).subarray(0, 5).toString()).toBe('%PDF-');
  });

  test('a page outside the book downloads standalone; a stale "book" request for it carries the Q-5-48 notice', async ({ page }) => {
    const s = await startBook();
    await page.goto(s.url);
    await openFile(page, 'other.qmd');

    // The menu offers no book entries on a non-chapter; the plain entry is the page alone.
    await page.getByRole('button', { name: 'Download as' }).click();
    await expect(page.getByRole('menuitem', { name: /Download book as/i })).toHaveCount(0);
    await page.keyboard.press('Escape');
    const plain = await download(page, /Typst.*\.typ\b/i);
    expect(plain.name).toBe('other.typ');
    expect(await readFile(plain.file, 'utf8')).toContain('DELTA');

    // A menu click that went stale (the page left the book after the menu opened) asks for `book`; Rust delivers the
    // page on its own and says so.
    const res = await page.request.post(new URL('api/preview/render', s.url).toString(), {
      data: { path: 'other.qmd', format: 'typst', content: BOOK_FILES[4].content, metadata: { 'book-scope': 'book' } },
    });
    expect(res.status()).toBe(200);
    expect(await res.text()).toContain('DELTA');
    const diagnostics = decodeURIComponent(res.headers()['x-q2-diagnostics'] ?? '');
    expect(diagnostics).toContain('Q-5-48');
    const stats = JSON.parse(decodeURIComponent(res.headers()['x-q2-stats'] ?? '{}'));
    expect(stats.book?.scope).not.toBe('book');
  });
});
