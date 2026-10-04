/**
 * EMF and WMF to PNG in a real browser (document import P4 T2/T6, epic I8 and I15): rtf.js's renderers
 * build an SVG with the DOM and it is rasterized through `<img>` and `<canvas>`, so this cannot run in
 * node. The `pandoc-*` prefix puts it in the WebKit project too, where SVG rasterization differs most.
 *
 *   VITE_E2E=1 npm run build
 *   npx playwright test -c playwright.harness.config.ts pandoc-import-emf
 */
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import type {} from './helpers/testHooks';
import { IMPORT_RECORDINGS, loadImportRecording } from '../src/test-utils/pandocRecordings';
import { validWmf } from '../src/test-utils/metafiles';

const outDir = process.env.Q2_EMF_OUT;

async function ready(page: Page) {
  await page.goto('/');
  await page.waitForFunction(() => !!window.__quartoTestReady);
  await page.evaluate(async () => {
    await window.__quartoTestReady;
  });
}

/** Convert in the page, then decode the PNG there and count the non-transparent pixels. */
async function convert(page: Page, bytes: Uint8Array, format: 'emf' | 'wmf') {
  return page.evaluate(
    async ({ base64, format }) => {
      const out = await window.__quartoTest!.pandoc.convertMetafile(base64, format);
      const png = Uint8Array.from(atob(out), (c) => c.charCodeAt(0));
      const bitmap = await createImageBitmap(new Blob([png], { type: 'image/png' }));
      const canvas = document.createElement('canvas');
      canvas.width = bitmap.width;
      canvas.height = bitmap.height;
      const ctx = canvas.getContext('2d')!;
      ctx.drawImage(bitmap, 0, 0);
      const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
      let painted = 0;
      for (let i = 3; i < data.length; i += 4) if (data[i] !== 0) painted++;
      return { base64: out, width: bitmap.width, height: bitmap.height, painted };
    },
    { base64: Buffer.from(bytes).toString('base64'), format },
  );
}

const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

test('the emf-docx fixture\'s EMF converts to a PNG with the rectangle drawn', async ({ page }) => {
  await ready(page);
  const emf = loadImportRecording('emf-docx').media.find((m) => m.rel.endsWith('.emf'))!;
  const r = await convert(page, emf.bytes, 'emf');
  const png = Buffer.from(r.base64, 'base64');
  expect([...png.subarray(0, 8)]).toEqual(PNG_SIGNATURE);
  // 100 x 50 px at 96 dpi (rclFrame 26.46 x 13.23 mm), at 2x.
  expect([r.width, r.height]).toEqual([200, 100]);
  expect(r.painted, 'a blank render has no painted pixel').toBeGreaterThan(0);
  if (outDir) {
    mkdirSync(outDir, { recursive: true });
    writeFileSync(path.join(outDir, `emf-${test.info().project.name}.png`), png);
  }
});

test('a spec-correct WMF converts to a PNG with the rectangle drawn', async ({ page }) => {
  await ready(page);
  const r = await convert(page, validWmf(), 'wmf');
  const png = Buffer.from(r.base64, 'base64');
  expect([...png.subarray(0, 8)]).toEqual(PNG_SIGNATURE);
  // 100 units at 1440 per inch is 0.069 in: 6.7 px at 96 dpi, 13 x 7 at 2x.
  expect([r.width, r.height]).toEqual([13, 7]);
  expect(r.painted, 'a blank render has no painted pixel').toBeGreaterThan(0);
  if (outDir) {
    mkdirSync(outDir, { recursive: true });
    writeFileSync(path.join(outDir, `wmf-${test.info().project.name}.png`), png);
  }
});

test('the emf-docx fixture\'s WMF (a 24-byte placeable header) is refused, so the import keeps the original', async ({ page }) => {
  await ready(page);
  const wmf = loadImportRecording('emf-docx').media.find((m) => m.rel.endsWith('.wmf'))!;
  const error = await page
    .evaluate(
      (base64) => window.__quartoTest!.pandoc.convertMetafile(base64, 'wmf').then(() => '', (e: Error) => e.message),
      Buffer.from(wmf.bytes).toString('base64'),
    );
  expect(error).toMatch(/WMF/);
});

/** Run the real import service in the page on a P1 fixture's source. */
async function importIn(page: Page, name: string) {
  const rec = loadImportRecording(name);
  return page.evaluate(
    async ({ base64, target }) => window.__quartoTest!.pandoc.importDocument(base64, 'source.docx', target),
    { base64: Buffer.from(rec.source).toString('base64'), target: `${name}.qmd` },
  );
}

test('importDocument imports basic-docx to its expected.qmd', async ({ page }) => {
  await ready(page);
  const out = await importIn(page, 'basic-docx');
  if (!out.ok) throw new Error(JSON.stringify(out.diagnostics));
  expect(out.qmd).toBe(readFileSync(path.join(IMPORT_RECORDINGS, 'basic-docx', 'expected.qmd'), 'utf8'));
  expect(out.media.length).toBeGreaterThan(0);
});

test('importDocument converts the emf-docx EMF to a PNG and keeps the malformed WMF as it is', async ({ page }) => {
  await ready(page);
  const out = await importIn(page, 'emf-docx');
  if (!out.ok) throw new Error(JSON.stringify(out.diagnostics));
  const png = out.media.find((m) => m.mimeType === 'image/png')!;
  expect(png, 'a PNG entry').toBeTruthy();
  expect(png.projectPath).toMatch(/^emf-docx_media\/[0-9a-f]{12}\.png$/);
  expect(out.qmd).toContain(png.projectPath);
  const bytes = Buffer.from(png.base64, 'base64');
  expect([...bytes.subarray(0, 8)]).toEqual(PNG_SIGNATURE);
  // The IHDR chunk's width and height.
  expect([bytes.readUInt32BE(16), bytes.readUInt32BE(20)]).toEqual([200, 100]);
  expect(out.media.some((m) => m.projectPath.endsWith('.wmf'))).toBe(true);
  const codes = out.diagnostics.map((d) => ('code' in d ? d.code : ''));
  expect(codes).toContain('Q-24-10');
  expect(codes).toContain('Q-24-9');
  // Decoded in the page, not blank.
  const painted = await page.evaluate(async (b64) => {
    const bitmap = await createImageBitmap(new Blob([Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))], { type: 'image/png' }));
    const canvas = document.createElement('canvas');
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    const ctx = canvas.getContext('2d')!;
    ctx.drawImage(bitmap, 0, 0);
    const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
    let n = 0;
    for (let i = 3; i < data.length; i += 4) if (data[i] !== 0) n++;
    return n;
  }, png.base64);
  expect(painted).toBeGreaterThan(0);
});

test('importDocument: the corrupt fixture gives Q-24-3', async ({ page }) => {
  await ready(page);
  expect(await importIn(page, 'corrupt-docx')).toMatchObject({ ok: false, diagnostics: [{ code: 'Q-24-3' }] });
});
