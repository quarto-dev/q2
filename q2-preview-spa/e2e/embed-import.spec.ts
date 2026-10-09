/**
 * hub-parity Plan 13: document import in `q2 preview --ui editor`, and through `--join`.
 *
 * The embed has no pandoc.wasm: the Import button posts the source to `POST /api/preview/import`, which runs the
 * machine's pandoc (sandboxed) and answers with the qmd and its media; the client then writes them into the
 * project through Automerge. Covered here:
 *   - a docx with an image: the qmd opens, `<stem>_media/` holds the image;
 *   - a docx with comments and tracked changes: they arrive as editorial marks;
 *   - a `--join` guest sees what the host imported, and a guest's own import (the POST crosses the tunnel and runs
 *     pandoc on the host) lands in the host's project, including a near-25 MB upload.
 *
 * Needs a `q2` built with the real embed (`cargo xtask build-hub-client-embed`, then `cargo build --bin q2`) and
 * pandoc on PATH. The `--join` cases also need network access to iroh's relay. Each missing prerequisite skips
 * with its reason.
 */

import { test, expect, type Page } from '@playwright/test';
import { existsSync, readdirSync } from 'node:fs';
import { chmod, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import { crc32, deflateSync } from 'node:zlib';
import { randomBytes } from 'node:crypto';
import path from 'node:path';
import { isEmbedBuilt, startJoinGuest, startPreviewServer, type JoinGuestHandle, type PreviewServerHandle } from './helpers/previewServer';

const REPO_ROOT = path.resolve(import.meta.dirname, '..', '..');
const RECORDINGS = path.join(REPO_ROOT, 'crates', 'quarto-core', 'tests', 'fixtures', 'import-recordings');

function onPath(cmd: string): boolean {
  try {
    execFileSync(cmd, ['--version'], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

const source = (recording: string) => readFile(path.join(RECORDINGS, recording, 'source.docx'));

let host: PreviewServerHandle | undefined;
let guest: JoinGuestHandle | undefined;
test.afterEach(async () => {
  await guest?.stop();
  await host?.stop();
  guest = host = undefined;
});

const INDEX = { path: 'index.qmd', content: '---\ntitle: Existing\n---\n\nAn existing document.\n' };

async function startHost(share: boolean, extraEnv?: Record<string, string>, allowEdit = false): Promise<PreviewServerHandle> {
  test.skip(!onPath('pandoc'), 'pandoc is not on PATH');
  host = await startPreviewServer({ fixtureFiles: [INDEX], ui: 'editor', share, extraEnv, allowEdit });
  test.skip(!(await isEmbedBuilt(host)), 'q2 was built without the hub-client embed (placeholder)');
  return host;
}

const dialog = (page: Page) => page.getByRole('dialog');
/** A file or folder row; a folder's accessible name starts with its disclosure arrow. */
const treeItem = (page: Page, name: string | RegExp) =>
  page.getByRole('treeitem', { name: typeof name === 'string' ? new RegExp(`^(?:[▶▼]\\s*)?${name.replace(/\./g, '\\.')}`) : name }).first();

async function openProject(page: Page, url: string): Promise<void> {
  await page.goto(url);
  await expect(treeItem(page, 'index.qmd')).toBeVisible({ timeout: 90_000 });
}

/** Choose `file` in the Import button's picker, press Import, and wait for the dialog to close (or its report to show). */
async function importFile(page: Page, name: string, buffer: Buffer, timeoutMs = 150_000): Promise<{ elapsed: number; report: string }> {
  await page.getByTestId('import-file-input').setInputFiles({ name, mimeType: 'application/octet-stream', buffer });
  await expect(dialog(page).getByLabel('Name:')).toBeVisible({ timeout: 30_000 });
  const started = Date.now();
  await dialog(page).getByRole('button', { name: 'Import', exact: true }).click();
  const report = page.getByTestId('import-report');
  await expect(async () => {
    expect((await dialog(page).count()) === 0 || (await report.isVisible())).toBe(true);
  }).toPass({ timeout: timeoutMs });
  const elapsed = Date.now() - started;
  let text = '';
  if (await report.isVisible()) {
    text = await report.innerText();
    await dialog(page).locator('.qh-btn.primary').click();
    await expect(dialog(page)).toHaveCount(0);
  }
  return { elapsed, report: text };
}

const editorText = (page: Page) => page.locator('.monaco-editor .view-lines').first();

/** A valid RGB PNG of about `bytes` bytes of incompressible data (stored deflate blocks), so a docx holding it is as big. */
function bigPng(bytes: number): Buffer {
  const width = 2000;
  const rowBytes = 1 + width * 3;
  const height = Math.ceil(bytes / rowBytes);
  const raw = Buffer.alloc(rowBytes * height);
  randomBytes(raw.length).copy(raw);
  for (let y = 0; y < height; y++) raw[y * rowBytes] = 0; // filter type: none
  const chunk = (type: string, data: Buffer): Buffer => {
    const length = Buffer.alloc(4);
    length.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body));
    return Buffer.concat([length, body, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr.set([8, 2, 0, 0, 0], 8); // 8-bit RGB
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 0 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

/**
 * A docx (made by this machine's pandoc) holding `count` distinct images of about `each` bytes. Each stays under
 * hub-client's 10 MB stored-image limit (`FILE_SIZE_LIMITS.MAX_FILE_SIZE`, which skips a larger image on import, in
 * the browser as well), while the docx as a whole is near the route's 25 MiB cap.
 */
async function docxWithImages(count: number, each: number): Promise<Buffer> {
  const dir = await mkdtemp(path.join(tmpdir(), 'q2-embed-bigdocx-'));
  let md = '# Big import\n\nBefore the images.\n\n';
  for (let i = 0; i < count; i++) {
    await writeFile(path.join(dir, `big${i}.png`), bigPng(each));
    md += `![Picture ${i}](big${i}.png)\n\n`;
  }
  await writeFile(path.join(dir, 'big.md'), md);
  execFileSync('pandoc', ['big.md', '-o', 'big.docx'], { cwd: dir });
  return readFile(path.join(dir, 'big.docx'));
}

test.describe('q2 preview --ui editor: import', () => {
  test('a docx with an image: the qmd opens and the image is stored under <stem>_media/', async ({ page }) => {
    const s = await startHost(false);
    await openProject(page, s.url);
    await importFile(page, 'basic-docx.docx', await source('basic-docx'));
    await expect(treeItem(page, 'basic-docx.qmd')).toBeVisible();
    await expect(editorText(page)).toContainText('First heading', { timeout: 30_000 });
    await expect(editorText(page)).toContainText('basic-docx_media/');
    await treeItem(page, 'basic-docx_media').click();
    await expect(treeItem(page, /\.png/)).toBeVisible();
  });

  test('a near-25 MB docx imports on the host itself (no tunnel): its images are stored', async ({ page }, testInfo) => {
    test.setTimeout(300_000);
    const s = await startHost(false);
    const docx = await docxWithImages(3, 7_000_000);
    await openProject(page, s.url);
    const { elapsed, report } = await importFile(page, 'big.docx', docx, 240_000);
    testInfo.annotations.push({ type: 'upload', description: `${docx.length} bytes, import dialog open for ${elapsed} ms; report: ${report || '(none)'}` });
    await expect(treeItem(page, 'big.qmd')).toBeVisible();
    await expect(treeItem(page, 'big_media')).toBeVisible({ timeout: 60_000 });
    await treeItem(page, 'big_media').click();
    await expect(page.getByRole('treeitem', { name: /\.png/ })).toHaveCount(3);
  });

  test('without --allow-edit an import lives in the session only; with it the qmd and its media are written to the project', async ({ page, browser }) => {
    test.setTimeout(180_000);
    const readOnly = await startHost(false);
    await openProject(page, readOnly.url);
    await importFile(page, 'basic-docx.docx', await source('basic-docx'));
    await expect(treeItem(page, 'basic-docx.qmd')).toBeVisible();
    // Give a write-back time to happen, then check that none did.
    await page.waitForTimeout(3_000);
    expect(existsSync(path.join(readOnly.projectDir, 'basic-docx.qmd'))).toBe(false);
    await host?.stop();

    const editable = await startHost(false, undefined, true);
    const page2 = await browser.newPage();
    try {
      await openProject(page2, editable.url);
      await importFile(page2, 'basic-docx.docx', await source('basic-docx'));
      await expect.poll(() => existsSync(path.join(editable.projectDir, 'basic-docx.qmd')), { timeout: 30_000 }).toBe(true);
      await expect
        .poll(() => (existsSync(path.join(editable.projectDir, 'basic-docx_media')) ? readdirSync(path.join(editable.projectDir, 'basic-docx_media')).length : 0), { timeout: 30_000 })
        .toBeGreaterThan(0);
    } finally {
      await page2.close();
    }
  });

  test('a docx with comments and tracked changes arrives as editorial marks', async ({ page }) => {
    const s = await startHost(false);
    await openProject(page, s.url);
    await importFile(page, 'track-changes-docx.docx', await source('track-changes-docx'));
    await expect(editorText(page)).toContainText('[++ ', { timeout: 30_000 });
    await expect(editorText(page)).toContainText('[-- ');
    await expect(editorText(page)).toContainText('[>> ');
  });
});

test.describe('q2 preview --ui editor --share: import through --join', () => {
  test('a guest sees what the host imported', async ({ page, browser }) => {
    test.setTimeout(240_000);
    const s = await startHost(true);
    guest = await startJoinGuest(await s.joinTicket());
    await openProject(page, s.url);
    const guestPage = await browser.newPage();
    try {
      await openProject(guestPage, guest.url);
      await importFile(page, 'basic-docx.docx', await source('basic-docx'));
      await expect(treeItem(page, 'basic-docx.qmd')).toBeVisible();
      await expect(treeItem(guestPage, 'basic-docx.qmd')).toBeVisible({ timeout: 90_000 });
      await expect(treeItem(guestPage, 'basic-docx_media')).toBeVisible({ timeout: 30_000 });
    } finally {
      await guestPage.close();
    }
  });

  test('a guest imports a near-25 MB docx: the upload crosses the tunnel, pandoc runs on the host, the host sees the result', async ({ page, browser }, testInfo) => {
    test.setTimeout(600_000);
    const s = await startHost(true);
    guest = await startJoinGuest(await s.joinTicket());
    const docx = await docxWithImages(3, 7_000_000);
    // The route's cap is 25 MiB; this is the near-limit case, not an over-limit one.
    expect(docx.length).toBeGreaterThan(20_000_000);
    expect(docx.length).toBeLessThan(25 * 1024 * 1024);
    await openProject(page, s.url);
    const guestPage = await browser.newPage();
    try {
      await openProject(guestPage, guest.url);
      const { elapsed, report } = await importFile(guestPage, 'big.docx', docx, 420_000);
      testInfo.annotations.push({ type: 'upload', description: `${docx.length} bytes, import dialog open for ${elapsed} ms; report: ${report || '(none)'}` });
      await expect(treeItem(guestPage, 'big.qmd')).toBeVisible({ timeout: 60_000 });
      await expect(treeItem(page, 'big.qmd')).toBeVisible({ timeout: 180_000 });
      await expect(treeItem(page, 'big_media')).toBeVisible({ timeout: 60_000 });
      await treeItem(page, 'big_media').click();
      await expect(page.getByRole('treeitem', { name: /\.png/ })).toHaveCount(3, { timeout: 180_000 });
    } finally {
      await guestPage.close();
    }
  });

  test('a slow import keeps the tunnel open: the guest waits out a 12 s pandoc and still gets its files', async ({ page, browser }, testInfo) => {
    test.setTimeout(240_000);
    test.skip(process.platform === 'win32', 'the slow pandoc is a shell script');
    // iroh's QUIC keep-alive (5 s by default) is what holds the idle tunnel open while the host's pandoc runs; the
    // tunnel itself sets none. A wrapper that sleeps before the real pandoc (but answers `--version` at once, for the
    // route's version gate) makes the idle stretch longer than that interval.
    const real = execFileSync('which', ['pandoc'], { encoding: 'utf8' }).trim();
    const bin = await mkdtemp(path.join(tmpdir(), 'q2-embed-slowpandoc-'));
    const wrapper = path.join(bin, 'pandoc');
    await writeFile(wrapper, `#!/bin/sh\ncase "$1" in --version|-v) ;; *) sleep 12 ;; esac\nexec "${real}" "$@"\n`);
    await chmod(wrapper, 0o755);
    const s = await startHost(true, { PATH: `${bin}${path.delimiter}${process.env.PATH ?? ''}` });
    guest = await startJoinGuest(await s.joinTicket());
    await openProject(page, s.url);
    const guestPage = await browser.newPage();
    try {
      await openProject(guestPage, guest.url);
      const { elapsed } = await importFile(guestPage, 'basic-docx.docx', await source('basic-docx'), 120_000);
      testInfo.annotations.push({ type: 'slow import', description: `${elapsed} ms` });
      expect(elapsed).toBeGreaterThan(10_000);
      await expect(treeItem(guestPage, 'basic-docx.qmd')).toBeVisible({ timeout: 60_000 });
      await expect(treeItem(page, 'basic-docx.qmd')).toBeVisible({ timeout: 60_000 });
    } finally {
      await guestPage.close();
    }
  });
});
