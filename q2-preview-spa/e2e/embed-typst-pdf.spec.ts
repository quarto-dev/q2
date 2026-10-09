/**
 * hub-parity Plan 12: `q2 preview --ui editor` (the hub-client embed) over the server's native routes.
 *
 * The embed has no pandoc.wasm, so its preview pane and "Download as" ask `POST /api/preview/pdf` and
 * `POST /api/preview/render`, which run the machine's pandoc and typst. Covered here:
 *   - a `format: typst` document shows its PDF in the preview pane, an edit re-renders it, and a broken edit
 *     keeps the last PDF under an error banner;
 *   - odt, PDF and typst-source downloads succeed;
 *   - a docx download of a code cell carries the cell's executed output (echo-engine capture, deno-gated).
 *
 * Needs a `q2` built with the real embed (`cargo xtask build-hub-client-embed`, then `cargo build --bin q2`),
 * and pandoc and typst on PATH. Each missing prerequisite skips with its reason.
 */

import { test, expect, type Page } from '@playwright/test';
import { cp, mkdtemp, mkdir, readFile, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { isEmbedBuilt, startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

const REPO_ROOT = path.resolve(import.meta.dirname, '..', '..');
const ECHO_ENGINE_FIXTURE = path.join(REPO_ROOT, 'crates', 'quarto-core', 'tests', 'fixtures', 'extensions', 'echo-engine');

function onPath(cmd: string, arg = '--version'): boolean {
  try {
    execFileSync(cmd, [arg], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

function onPathIn(cmd: string, searchPath: string): boolean {
  try {
    execFileSync(cmd, ['--version'], { stdio: 'ignore', env: { PATH: searchPath } });
    return true;
  } catch {
    return false;
  }
}

const TYPST_QMD = (body: string) => `---\ntitle: Embed PDF\nformat: typst\n---\n\n# Heading\n\n${body}\n`;

let server: PreviewServerHandle | undefined;
test.afterEach(async () => {
  await server?.stop();
  server = undefined;
});

/** Start the editor UI over `files`; skips the test when the binary holds the placeholder embed. */
async function startEditor(files: { path: string; content: string }[]): Promise<PreviewServerHandle> {
  test.skip(!onPath('pandoc'), 'pandoc is not on PATH');
  test.skip(!onPath('typst'), 'typst is not on PATH');
  server = await startPreviewServer({ fixtureFiles: files, ui: 'editor' });
  test.skip(!(await isEmbedBuilt(server)), 'q2 was built without the hub-client embed (placeholder)');
  return server;
}

const pdfFrame = (page: Page) => page.locator('[data-testid="pdf-preview-pane"] iframe[title="PDF preview"]');

/** The viewer's own page text (pdf.js text layer), once it has drawn. */
async function pdfText(page: Page): Promise<string> {
  const frame = page.frameLocator('[data-testid="pdf-preview-pane"] iframe[title="PDF preview"]').first();
  await frame.locator('.textLayer span').first().waitFor({ timeout: 60_000 });
  return (await frame.locator('.textLayer').first().innerText()) ?? '';
}

async function download(page: Page, label: RegExp): Promise<{ file: string; name: string }> {
  await page.getByRole('button', { name: 'Download as' }).click();
  const [dl] = await Promise.all([page.waitForEvent('download', { timeout: 90_000 }), page.getByRole('menuitem', { name: label }).click()]);
  const dir = await mkdtemp(path.join(tmpdir(), 'q2-embed-dl-'));
  const file = path.join(dir, dl.suggestedFilename());
  await dl.saveAs(file);
  return { file, name: dl.suggestedFilename() };
}

test.describe('q2 preview --ui editor: typst PDF', () => {
  test('the PDF pane appears, an edit re-renders, a broken edit keeps the last PDF under a banner', async ({ page }) => {
    const s = await startEditor([{ path: 'index.qmd', content: TYPST_QMD('First version token ALPHA.') }]);
    await page.goto(s.url);
    await expect(page.getByTestId('pdf-preview-pane')).toBeVisible({ timeout: 60_000 });
    await expect(pdfFrame(page)).toBeAttached({ timeout: 90_000 });
    expect(await pdfText(page)).toContain('ALPHA');

    await writeFile(path.join(s.projectDir, 'index.qmd'), TYPST_QMD('Second version token BETA.'));
    await expect.poll(async () => (await pdfText(page).catch(() => '')).includes('BETA'), { timeout: 60_000 }).toBe(true);

    // A raw typst block with a syntax error: pandoc succeeds, the typst compile fails.
    await writeFile(path.join(s.projectDir, 'index.qmd'), TYPST_QMD('```{=typst}\n#let x = (\n```'));
    await expect(page.getByTestId('pdf-preview-error')).toBeVisible({ timeout: 60_000 });
    // The last good PDF stays on screen.
    await expect(pdfFrame(page)).toBeAttached();
    expect(await pdfText(page)).toContain('BETA');
  });

  test('odt, PDF and typst source downloads succeed', async ({ page }) => {
    const s = await startEditor([{ path: 'index.qmd', content: TYPST_QMD('Download token GAMMA.') }]);
    await page.goto(s.url);
    await expect(page.getByTestId('pdf-preview-pane')).toBeVisible({ timeout: 60_000 });

    const pdf = await download(page, /\.pdf/i);
    expect(pdf.name).toMatch(/\.pdf$/);
    expect((await readFile(pdf.file)).subarray(0, 5).toString()).toBe('%PDF-');

    const odt = await download(page, /\.odt/i);
    expect(odt.name).toMatch(/\.odt$/);
    expect((await readFile(odt.file)).subarray(0, 2).toString()).toBe('PK');

    const typ = await download(page, /\.typ\b/i);
    expect(typ.name).toMatch(/\.typ$/);
    expect(await readFile(typ.file, 'utf8')).toContain('GAMMA');
  });
});

test.describe('q2 preview --ui editor: missing typst', () => {
  test('the pane banner names the missing compiler (Q-21-1) instead of staying blank', async ({ page }) => {
    test.skip(!onPath('pandoc'), 'pandoc is not on PATH');
    // A PATH holding pandoc (as a symlink) but no typst.
    const bin = await mkdtemp(path.join(tmpdir(), 'q2-embed-nopath-'));
    const pandoc = execFileSync('which', ['pandoc'], { encoding: 'utf8' }).trim();
    await symlink(pandoc, path.join(bin, 'pandoc'));
    server = await startPreviewServer({
      fixtureFiles: [{ path: 'index.qmd', content: TYPST_QMD('No compiler.') }],
      ui: 'editor',
      extraEnv: { PATH: `${bin}${path.delimiter}/usr/bin${path.delimiter}/bin` },
    });
    test.skip(!(await isEmbedBuilt(server)), 'q2 was built without the hub-client embed (placeholder)');
    test.skip(onPathIn('typst', `/usr/bin${path.delimiter}/bin`), 'typst is installed in a system directory');
    await page.goto(server.url);
    const banner = page.getByTestId('pdf-preview-error');
    await expect(banner).toBeVisible({ timeout: 60_000 });
    await expect(banner).toContainText('Q-21-1');
  });
});

test.describe('q2 preview --ui editor: code-cell output in a docx', () => {
  test('a docx download carries the executed output of an engine cell', async ({ page }) => {
    test.setTimeout(150_000);
    test.skip(!onPath('deno'), 'deno is not on PATH (the echo engine-host runtime)');
    test.skip(!onPath('pandoc'), 'pandoc is not on PATH');
    const projSrc = await mkdtemp(path.join(tmpdir(), 'q2-embed-echo-src-'));
    await mkdir(path.join(projSrc, '_extensions'), { recursive: true });
    const ext = path.join(projSrc, '_extensions', 'echo-engine');
    await cp(ECHO_ENGINE_FIXTURE, ext, { recursive: true });
    // The fixture ships source only; the engine host needs the bundled `dist/echo-engine.js`.
    execFileSync(path.join(REPO_ROOT, 'target', 'debug', 'q2'), ['call', 'build-ts-extension', ext], { stdio: 'ignore' });
    await writeFile(path.join(projSrc, 'index.qmd'), '---\ntitle: Echo\n---\n\n# Echo heading\n\n```{echo}\nEMBED_ECHO_SOURCE\n```\n');
    server = await startPreviewServer({ copyFromDir: projSrc, ui: 'editor', extraEnv: { RUST_LOG: 'warn' } });
    test.skip(!(await isEmbedBuilt(server)), 'q2 was built without the hub-client embed (placeholder)');
    await page.goto(server.url);
    await expect(page.getByRole('button', { name: 'Download as' })).toBeVisible({ timeout: 60_000 });
    // The capture is recorded and synced asynchronously: retry the download until the output is in the file.
    await expect
      .poll(
        async () => {
          const docx = await download(page, /\.docx/i);
          const xml = execFileSync('unzip', ['-p', docx.file, 'word/document.xml'], { encoding: 'utf8', maxBuffer: 1 << 26 });
          return xml.includes('ECHO_EXECUTED') && !xml.includes('EMBED_ECHO_SOURCE');
        },
        { timeout: 90_000, intervals: [2_000] },
      )
      .toBe(true);
  });
});
