/**
 * SVG images in a browser docx/pptx download (metafile-svg plan T2-T4): pandoc.wasm has no
 * `rsvg-convert`, so a docx/pptx writer would show an SVG as alt text. The Rust
 * `RasterizeSvgImagesStage` asks the main-thread rasterizer (`wasm-js-bridge/rasterize.js`, an
 * `<img>` drawn onto a `<canvas>`) for a PNG before pandoc runs. Node cannot rasterize, so this is
 * the gate: the real wasm, the real rasterizer, then pandoc.wasm's own docx/pptx writer.
 *
 * Runs in Chromium, Firefox and WebKit (the `pandoc-*` prefix): they disagree on the size of a
 * viewBox-only SVG and on whether `<foreignObject>` taints the canvas.
 *
 * Needs a VITE_E2E=1 build with hub-client/public/pandoc/pandoc.wasm.gz (`npm run test:harness`).
 */
import { mkdtempSync, readFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { unzipSync } from 'fflate';
import { expect, test, type Page } from '@playwright/test';
import type {} from './helpers/testHooks';
import { loadImportRecording } from '../src/test-utils/pandocRecordings';

/** 100 x 50 CSS px by its viewBox alone: browsers disagree on what that means (Chromium 300x150, WebKit 100x50). */
const VIEWBOX_ONLY = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50"><rect width="100" height="50" fill="#d00"/></svg>';
/** 2 in x 1 in. */
const SIZED = '<svg xmlns="http://www.w3.org/2000/svg" width="2in" height="1in" viewBox="0 0 10 5"><rect width="10" height="5" fill="#00d"/></svg>';
/** `<foreignObject>` taints the canvas in Chromium and WebKit, so `toBlob` throws there. */
const FOREIGN = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50"><foreignObject width="100" height="50"><div xmlns="http://www.w3.org/1999/xhtml">hi</div></foreignObject></svg>';

const EMU_PER_INCH = 914400;

async function download(page: Page, format: 'docx' | 'pptx' | 'odt', svgs: Record<string, string>, qmd: string) {
  await page.goto('/');
  await page.waitForFunction(() => !!window.__quartoTestReady);
  await page.evaluate(async () => {
    await window.__quartoTestReady;
  });
  const downloaded = page.waitForEvent('download');
  const summary = await page.evaluate(
    async ({ qmd, format, svgs }) => {
      const { wasmRenderer, pandoc } = window.__quartoTest!;
      await wasmRenderer.initWasm();
      wasmRenderer.vfsClear();
      wasmRenderer.vfsAddFile('/project/doc.qmd', qmd);
      for (const [name, text] of Object.entries(svgs)) wasmRenderer.vfsAddBinaryFile(`/project/${name}`, new TextEncoder().encode(text));
      // The production controller: its warnings include the request build's (the rasterizer's).
      const r = await pandoc.startDownload('/project/doc.qmd', format);
      return { ok: r.phase === 'done', phase: r.phase, diagnostics: r.diagnostics };
    },
    { qmd, format, svgs },
  );
  expect(summary.ok, JSON.stringify(summary)).toBe(true);
  const file = path.join(mkdtempSync(path.join(os.tmpdir(), 'q2-svg-raster-')), `out.${format}`);
  await (await downloaded).saveAs(file);
  return { entries: unzipSync(new Uint8Array(readFileSync(file))), summary };
}

const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/** Width, height and pHYs (pixels per metre, x) of a PNG, read from its chunks. */
function pngInfo(png: Uint8Array) {
  expect([...png.subarray(0, 8)]).toEqual(PNG_SIGNATURE);
  const v = new DataView(png.buffer, png.byteOffset, png.byteLength);
  let phys: number | undefined;
  for (let o = 8; o + 8 <= png.length; ) {
    const len = v.getUint32(o);
    const type = String.fromCharCode(...png.subarray(o + 4, o + 8));
    if (type === 'pHYs') phys = v.getUint32(o + 8);
    o += 12 + len;
  }
  return { width: v.getUint32(16), height: v.getUint32(20), phys };
}

/** Decode in the page and count the opaque pixels, so a blank canvas fails. */
async function painted(page: Page, png: Uint8Array): Promise<number> {
  return page.evaluate(async (base64) => {
    const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
    const bitmap = await createImageBitmap(new Blob([bytes], { type: 'image/png' }));
    const canvas = document.createElement('canvas');
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    const ctx = canvas.getContext('2d')!;
    ctx.drawImage(bitmap, 0, 0);
    const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
    let n = 0;
    for (let i = 3; i < data.length; i += 4) if (data[i] !== 0) n++;
    return n;
  }, Buffer.from(png).toString('base64'));
}

const mediaPngs = (entries: Record<string, Uint8Array>, dir: string) =>
  Object.entries(entries).filter(([name]) => name.startsWith(dir) && name.endsWith('.png'));

const DOC = '---\ntitle: SVG\n---\n\n![viewbox only](a.svg)\n\n![sized](b.svg)\n';

test('docx embeds a user-authored SVG as a PNG, shown at the SVG\'s own size', async ({ page }) => {
  const { entries, summary } = await download(page, 'docx', { 'a.svg': VIEWBOX_ONLY, 'b.svg': SIZED }, DOC);
  expect(Object.keys(entries).filter((n) => n.endsWith('.svg')), `no SVG is embedded; ${JSON.stringify(summary.diagnostics)}`).toEqual([]);
  const pngs = mediaPngs(entries, 'word/media/');
  expect(pngs.length).toBe(2);

  const infos = pngs.map(([, bytes]) => pngInfo(bytes));
  // 2x the intrinsic size: 100x50 CSS px and 2in x 1in (192x96 CSS px).
  expect(infos.map((i) => [i.width, i.height]).sort()).toEqual([[200, 100], [384, 192]].sort());
  // 192 dpi to pandoc (it truncates the dpi, so 7560 px/m), so it displays the PNG at the SVG's size, not 2.7x too large.
  for (const i of infos) expect(i.phys).toBe(7560);
  for (const [, bytes] of pngs) expect(await painted(page, bytes), 'a blank canvas has no opaque pixel').toBeGreaterThan(0);

  const xml = new TextDecoder().decode(entries['word/document.xml']);
  expect((xml.match(/<a:blip [^>]*r:embed="[^"]+"/g) ?? []).length).toBe(2);
  const extents = [...xml.matchAll(/<wp:extent cx="(\d+)" cy="(\d+)"/g)].map((m) => [Number(m[1]), Number(m[2])]);
  // 100x50 px at 96 dpi, then 2 in x 1 in; allow a rounding EMU.
  const want = [[Math.round((100 / 96) * EMU_PER_INCH), Math.round((50 / 96) * EMU_PER_INCH)], [2 * EMU_PER_INCH, EMU_PER_INCH]];
  expect(extents.length).toBe(2);
  extents.forEach(([cx, cy], i) => {
    expect(Math.abs(cx - want[i][0])).toBeLessThanOrEqual(1);
    expect(Math.abs(cy - want[i][1])).toBeLessThanOrEqual(1);
  });
  expect(JSON.stringify(summary.diagnostics)).not.toContain('SVG image');
});

test('pptx embeds a user-authored SVG as a PNG', async ({ page }) => {
  const { entries } = await download(page, 'pptx', { 'a.svg': VIEWBOX_ONLY }, '---\ntitle: SVG\n---\n\n## Slide\n\n![viewbox only](a.svg)\n');
  expect(Object.keys(entries).filter((n) => n.endsWith('.svg'))).toEqual([]);
  const pngs = mediaPngs(entries, 'ppt/media/');
  expect(pngs.length).toBe(1);
  expect(pngInfo(pngs[0][1]).width).toBe(200);
  expect(await painted(page, pngs[0][1])).toBeGreaterThan(0);
  const slide = new TextDecoder().decode(entries['ppt/slides/slide2.xml'] ?? entries['ppt/slides/slide1.xml']);
  expect(slide).toMatch(/<a:blip [^>]*r:embed="[^"]+"/);
});

test('an SVG the canvas refuses still downloads, with a warning where the browser taints it', async ({ page }) => {
  const { entries, summary } = await download(page, 'docx', { 'f.svg': FOREIGN }, '---\ntitle: F\n---\n\n![fo](f.svg)\n');
  const pngs = mediaPngs(entries, 'word/media/');
  const warned = JSON.stringify(summary.diagnostics).includes('SVG image');
  if (test.info().project.name === 'firefox') {
    // Firefox does not taint the canvas for <foreignObject>: it rasterizes.
    expect(pngs.length).toBe(1);
    expect(warned).toBe(false);
  } else {
    // Chromium and WebKit throw a SecurityError from toBlob: the SVG stays, pandoc shows the alt text.
    expect(pngs.length).toBe(0);
    expect(warned, JSON.stringify(summary.diagnostics)).toBe(true);
    expect(new TextDecoder().decode(entries['word/document.xml'])).toContain('fo');
  }
});

test('an imported EMF (stored as SVG) is exported to docx as a PNG at the EMF\'s own size', async ({ page }) => {
  await page.goto('/');
  await page.waitForFunction(() => !!window.__quartoTestReady);
  await page.evaluate(async () => {
    await window.__quartoTestReady;
  });
  const source = Buffer.from(loadImportRecording('emf-docx').source).toString('base64');
  const downloaded = page.waitForEvent('download');
  const summary = await page.evaluate(
    async (base64) => {
      const { wasmRenderer, pandoc } = window.__quartoTest!;
      // The production import: the EMF comes back as `<hash>.svg`, which the project would store as is.
      const imported = await pandoc.importDocument(base64, 'source.docx', 'emf-docx.qmd');
      if (!imported.ok) return { error: JSON.stringify(imported.diagnostics) };
      const svg = imported.media.find((m) => m.mimeType === 'image/svg+xml');
      if (!svg) return { error: 'the import stored no SVG' };
      await wasmRenderer.initWasm();
      wasmRenderer.vfsClear();
      wasmRenderer.vfsAddFile('/project/doc.qmd', `---\ntitle: EMF\n---\n\n![emf](${svg.projectPath})\n`);
      wasmRenderer.vfsAddBinaryFile(`/project/${svg.projectPath}`, Uint8Array.from(atob(svg.base64), (c) => c.charCodeAt(0)));
      const r = await pandoc.startDownload('/project/doc.qmd', 'docx');
      return { ok: r.phase === 'done', phase: r.phase, diagnostics: r.diagnostics };
    },
    source,
  );
  expect(summary, JSON.stringify(summary)).toMatchObject({ ok: true });
  const file = path.join(mkdtempSync(path.join(os.tmpdir(), 'q2-svg-raster-')), 'out.docx');
  await (await downloaded).saveAs(file);
  const entries = unzipSync(new Uint8Array(readFileSync(file)));

  expect(Object.keys(entries).filter((n) => n.endsWith('.svg') || n.endsWith('.emf'))).toEqual([]);
  const pngs = mediaPngs(entries, 'word/media/');
  expect(pngs.length).toBe(1);
  // The fixture EMF is 100 x 50 px at 96 dpi; rasterized at 2x.
  expect([pngInfo(pngs[0][1]).width, pngInfo(pngs[0][1]).height]).toEqual([200, 100]);
  expect(await painted(page, pngs[0][1]), 'a blank canvas has no opaque pixel').toBeGreaterThan(0);
  const xml = new TextDecoder().decode(entries['word/document.xml']);
  expect((xml.match(/<a:blip [^>]*r:embed="[^"]+"/g) ?? []).length).toBe(1);
  const m = xml.match(/<wp:extent cx="(\d+)" cy="(\d+)"/)!;
  expect(Math.abs(Number(m[1]) - Math.round((100 / 96) * EMU_PER_INCH))).toBeLessThanOrEqual(1);
  expect(Math.abs(Number(m[2]) - Math.round((50 / 96) * EMU_PER_INCH))).toBeLessThanOrEqual(1);
});

test('typst keeps the SVG: nothing is rasterized for a format that reads SVG', async ({ page }) => {
  await page.goto('/');
  await page.waitForFunction(() => !!window.__quartoTestReady);
  await page.evaluate(async () => {
    await window.__quartoTestReady;
  });
  const out = await page.evaluate(
    async ({ qmd, svg }) => {
      const { wasmRenderer } = window.__quartoTest!;
      await wasmRenderer.initWasm();
      wasmRenderer.vfsClear();
      wasmRenderer.vfsAddFile('/project/doc.qmd', qmd);
      wasmRenderer.vfsAddBinaryFile('/project/a.svg', new TextEncoder().encode(svg));
      const envelope = await wasmRenderer.renderPandocRequest('/project/doc.qmd', 'typst', { sourceDateEpoch: 1_700_000_000 });
      const request = envelope.request as unknown as { resource_refs: { path: string }[] } | undefined;
      return request?.resource_refs.map((f) => f.path) ?? null;
    },
    { qmd: '---\ntitle: T\n---\n\n![x](a.svg)\n', svg: VIEWBOX_ONLY },
  );
  expect(out, 'a request was built').not.toBeNull();
  expect(out!.some((p) => p.includes('/_raster/'))).toBe(false);
});

/** What an odt holds for its images: the Pictures/ entries and each `draw:frame`'s size and `draw:image` href. */
function odtImages(entries: Record<string, Uint8Array>) {
  const xml = new TextDecoder().decode(entries['content.xml']);
  const frames = [...xml.matchAll(/<draw:frame\b[^>]*?svg:width="([^"]+)"[^>]*?svg:height="([^"]+)"[^>]*>(.*?)<\/draw:frame>/gs)].map((m) => ({
    width: m[1],
    height: m[2],
    href: m[3].match(/<draw:image\b[^>]*xlink:href="([^"]+)"/)?.[1],
  }));
  return { xml, pictures: Object.keys(entries).filter((n) => n.startsWith('Pictures/') && !n.endsWith('/')), frames };
}

test('odt embeds a user-authored SVG as is, at the SVG\'s own size', async ({ page }) => {
  const { entries, summary } = await download(page, 'odt', { 'a.svg': VIEWBOX_ONLY, 'b.svg': SIZED }, DOC);
  const { pictures, frames } = odtImages(entries);
  // ODF supports SVG, so pandoc embeds it as is: no rasterize stage for odt (metafile-svg plan T6).
  expect(pictures.sort()).toEqual(['Pictures/0.svg', 'Pictures/1.svg']);
  expect(frames.map((f) => [f.width, f.height, f.href])).toEqual([
    ['75.0pt', '37.5pt', 'Pictures/0.svg'], // 100 x 50 px at 96 dpi
    ['144.0pt', '72.0pt', 'Pictures/1.svg'], // 2 in x 1 in
  ]);
  expect(new TextDecoder().decode(entries['META-INF/manifest.xml'])).toContain('media-type="image/svg+xml"');
  expect(JSON.stringify(summary.diagnostics)).not.toContain('SVG image');
});

test('an imported EMF (stored as SVG) is exported to odt as that SVG, at the EMF\'s own size', async ({ page }) => {
  await page.goto('/');
  await page.waitForFunction(() => !!window.__quartoTestReady);
  await page.evaluate(async () => {
    await window.__quartoTestReady;
  });
  const source = Buffer.from(loadImportRecording('emf-docx').source).toString('base64');
  const downloaded = page.waitForEvent('download');
  const summary = await page.evaluate(
    async (base64) => {
      const { wasmRenderer, pandoc } = window.__quartoTest!;
      const imported = await pandoc.importDocument(base64, 'source.docx', 'emf-docx.qmd');
      if (!imported.ok) return { error: JSON.stringify(imported.diagnostics) };
      const svg = imported.media.find((m) => m.mimeType === 'image/svg+xml');
      if (!svg) return { error: 'the import stored no SVG' };
      await wasmRenderer.initWasm();
      wasmRenderer.vfsClear();
      wasmRenderer.vfsAddFile('/project/doc.qmd', `---\ntitle: EMF\n---\n\n![emf](${svg.projectPath})\n`);
      wasmRenderer.vfsAddBinaryFile(`/project/${svg.projectPath}`, Uint8Array.from(atob(svg.base64), (c) => c.charCodeAt(0)));
      const r = await pandoc.startDownload('/project/doc.qmd', 'odt');
      return { ok: r.phase === 'done', phase: r.phase, diagnostics: r.diagnostics };
    },
    source,
  );
  expect(summary, JSON.stringify(summary)).toMatchObject({ ok: true });
  const file = path.join(mkdtempSync(path.join(os.tmpdir(), 'q2-svg-raster-')), 'out.odt');
  await (await downloaded).saveAs(file);
  const entries = unzipSync(new Uint8Array(readFileSync(file)));
  const { pictures, frames } = odtImages(entries);
  expect(pictures).toEqual(['Pictures/0.svg']);
  // The fixture EMF is 100 x 50 px at 96 dpi.
  expect(frames.map((f) => [f.width, f.height, f.href])).toEqual([['75.0pt', '37.5pt', 'Pictures/0.svg']]);
  expect(JSON.stringify(summary)).not.toContain('SVG image');
});
