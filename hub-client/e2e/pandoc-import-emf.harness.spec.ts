/**
 * EMF and WMF to SVG in a real browser (document import P4 T2/T6, metafile-svg plan T1): rtf.js's renderers
 * build an SVG with the DOM, and the checks load it through `<img>` and `<canvas>`, so this cannot run in
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

/** What the SVG may contain: the elements rtf.js's EMF/WMF renderers emit (T0 recorded the set), and nothing that runs. */
const ALLOWED = ['svg', 'defs', 'clippath', 'pattern', 'filter', 'feflood', 'fecomposite', 'image', 'rect', 'line', 'polygon', 'polyline', 'ellipse', 'path', 'text'];

/** Inspect an SVG's text in the page: its elements and hrefs, its root size, and the opaque pixels it draws. */
async function inspect(page: Page, svg: string) {
  return page.evaluate(async (text) => {
    const doc = new DOMParser().parseFromString(text, 'image/svg+xml');
    const root = doc.documentElement;
    const elements = [...new Set([root, ...root.querySelectorAll('*')].map((e) => e.localName.toLowerCase()))];
    const hrefs = [...root.querySelectorAll('*')].flatMap((e) => ['href', 'xlink:href'].map((n) => e.getAttribute(n)).filter((v): v is string => v !== null));
    const img = new Image();
    const loaded = new Promise<void>((resolve, reject) => {
      img.onload = () => resolve();
      img.onerror = () => reject(new Error('the SVG did not load through <img>'));
    });
    img.src = URL.createObjectURL(new Blob([text], { type: 'image/svg+xml' }));
    await loaded;
    const canvas = document.createElement('canvas');
    canvas.width = img.naturalWidth;
    canvas.height = img.naturalHeight;
    const ctx = canvas.getContext('2d')!;
    ctx.drawImage(img, 0, 0);
    const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
    let painted = 0;
    for (let i = 3; i < data.length; i += 4) if (data[i] !== 0) painted++;
    return { parsed: root.localName === 'svg', elements, hrefs, rootWidth: root.getAttribute('width'), rootHeight: root.getAttribute('height'), painted };
  }, svg);
}

async function convert(page: Page, bytes: Uint8Array, format: 'emf' | 'wmf') {
  return page.evaluate(
    ({ base64, format }) => window.__quartoTest!.pandoc.convertMetafile(base64, format),
    { base64: Buffer.from(bytes).toString('base64'), format },
  );
}

/** The converted SVG is non-blank, 1x, and holds only allowed elements and `data:` hrefs. */
async function expectSvg(page: Page, out: { svg: string; width: number; height: number }, size: [number, number]) {
  expect([out.width, out.height]).toEqual(size);
  const r = await inspect(page, out.svg);
  expect(r.parsed).toBe(true);
  expect([r.rootWidth, r.rootHeight]).toEqual([`${size[0]}px`, `${size[1]}px`]);
  expect(r.elements.filter((e) => !ALLOWED.includes(e)), 'elements outside the allowlist').toEqual([]);
  for (const href of r.hrefs) expect(href.startsWith('data:'), `href ${href.slice(0, 40)}`).toBe(true);
  expect(r.painted, 'a blank render has no painted pixel').toBeGreaterThan(0);
}

test('the emf-docx fixture\'s EMF converts to an SVG at 1x with the rectangle drawn', async ({ page }) => {
  await ready(page);
  const emf = loadImportRecording('emf-docx').media.find((m) => m.rel.endsWith('.emf'))!;
  const out = await convert(page, emf.bytes, 'emf');
  // 100 x 50 px at 96 dpi (rclFrame 26.46 x 13.23 mm).
  await expectSvg(page, out, [100, 50]);
  if (outDir) {
    mkdirSync(outDir, { recursive: true });
    writeFileSync(path.join(outDir, `emf-${test.info().project.name}.svg`), out.svg);
  }
});

test('a spec-correct WMF converts to an SVG at 1x with the rectangle drawn', async ({ page }) => {
  await ready(page);
  const out = await convert(page, validWmf(), 'wmf');
  // 100 units at 1440 per inch is 0.069 in: 6.7 px at 96 dpi, so 7 x 3.
  await expectSvg(page, out, [7, 3]);
  if (outDir) {
    mkdirSync(outDir, { recursive: true });
    writeFileSync(path.join(outDir, `wmf-${test.info().project.name}.svg`), out.svg);
  }
});

test('a clipped WMF converted twice in one page gives identical bytes (rtf.js\'s clip counter is renumbered away)', async ({ page }) => {
  await ready(page);
  const first = await convert(page, validWmf({ clip: true }), 'wmf');
  const second = await convert(page, validWmf({ clip: true }), 'wmf');
  // Without a clipPath the comparison proves nothing.
  expect(first.svg).toMatch(/<clipPath[^>]* id="wmfjs_c0"/);
  expect(second.svg).toBe(first.svg);
  await expectSvg(page, first, [7, 3]);
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

test('importDocument converts the emf-docx EMF to an SVG and keeps the malformed WMF as it is', async ({ page }) => {
  await ready(page);
  const out = await importIn(page, 'emf-docx');
  if (!out.ok) throw new Error(JSON.stringify(out.diagnostics));
  const svg = out.media.find((m) => m.mimeType === 'image/svg+xml')!;
  expect(svg, 'an SVG entry').toBeTruthy();
  expect(svg.projectPath).toMatch(/^emf-docx_media\/[0-9a-f]{12}\.svg$/);
  expect(out.qmd).toContain(svg.projectPath);
  const text = Buffer.from(svg.base64, 'base64').toString('utf8');
  expect(text.startsWith('<svg')).toBe(true);
  expect(out.media.some((m) => m.projectPath.endsWith('.wmf'))).toBe(true);
  const codes = out.diagnostics.map((d) => ('code' in d ? d.code : ''));
  expect(codes).toContain('Q-24-10');
  expect(codes).toContain('Q-24-9');
  const r = await inspect(page, text);
  expect([r.rootWidth, r.rootHeight]).toEqual(['100px', '50px']);
  expect(r.painted).toBeGreaterThan(0);
});

test('importDocument: the corrupt fixture gives Q-24-3', async ({ page }) => {
  await ready(page);
  expect(await importIn(page, 'corrupt-docx')).toMatchObject({ ok: false, diagnostics: [{ code: 'Q-24-3' }] });
});
