/**
 * Real-wasm tests for the import service (document import P4 T6): the production `createImportService`
 * with the built Rust wasm (`prepare_import` / `finish_import` / `classify_import_failure`) and pandoc.wasm
 * in a real worker thread, over every P1 fixture. Each import gives the fixture's `expected.qmd` and media
 * whose bytes match its `media/` directory.
 *
 * EMF/WMF conversion needs the DOM, so here the converter is injected: it fails (the original is stored,
 * `conversion_failed`, which is the manifest `expected.qmd` for `emf-docx` was written from) or returns a
 * canned PNG. The real converter runs in `e2e/pandoc-import-emf.harness.spec.ts`.
 *
 * Needs `node scripts/fetch-pandoc-wasm.mjs` and `npm run build:wasm`. Run with: npm run test:wasm
 */
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { FakeCache } from '../test-utils/fakeCache';
import { nodePandocWorker } from '../test-utils/nodeWorker';
import { IMPORT_RECORDINGS, WASM_PATH, importRecordingNames, loadImportRecording, pandocWasmAvailable } from '../test-utils/pandocRecordings';
import { FILE_SIZE_LIMITS } from '../services/resourceService';
import { createImportService, type ImportDeps, type ImportOutcome } from './importService';
import { PandocLoader } from './pandocLoader';
import { PandocRunner } from './pandocRunner';

const here = path.dirname(fileURLToPath(import.meta.url));
const CI = !!process.env.CI;
const BASE = 'https://app.test/';

const sha256 = (b: Uint8Array) => createHash('sha256').update(b).digest('hex');

it('has the pandoc.wasm asset (required in CI)', () => {
  if (!pandocWasmAvailable() && CI) throw new Error('pandoc.wasm missing: run node scripts/fetch-pandoc-wasm.mjs --require');
  if (!pandocWasmAvailable()) console.warn('SKIPPING the real-wasm import tests: run node scripts/fetch-pandoc-wasm.mjs');
});

interface Wasm {
  default: (input?: BufferSource) => Promise<void>;
  get_import_formats: () => string;
  prepare_import: (name: string, size: number, sha: string) => string;
  finish_import: (json: string, stderr: string, target: string, manifest: string, format?: string) => string;
  classify_import_failure: (kind: string, status: number | null | undefined, stderr: string) => string;
}

const expectedQmd = (name: string) => readFileSync(path.join(IMPORT_RECORDINGS, name, 'expected.qmd'), 'utf8');
const extOf = (n: string) => n.slice(n.lastIndexOf('-') + 1);
/** A canned PNG the fake converter returns. */
const FAKE_PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]);

describe.skipIf(!pandocWasmAvailable())('importDocument against the real wasm', () => {
  let wasm: Wasm;
  let gz: Uint8Array;
  beforeAll(async () => {
    wasm = (await import('wasm-quarto-hub-client')) as unknown as Wasm;
    await wasm.default(await readFile(path.resolve(here, '../../wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm')));
    gz = new Uint8Array(gzipSync(readFileSync(WASM_PATH), { level: 1 }));
  }, 120_000);

  /** One loader (so one compile) per call; `loadSpy` counts how often pandoc.wasm was asked for. */
  function make(convertImage: ImportDeps['convertImage'] = async () => Promise.reject(new Error('no DOM in node'))) {
    const caches = new Map<string, FakeCache>();
    const storage = { open: async (n: string) => caches.get(n) ?? caches.set(n, new FakeCache()).get(n) } as unknown as CacheStorage;
    const loader = new PandocLoader({
      env: {
        caches: storage,
        baseURI: BASE,
        fetch: (async (url: string) => (url.split('?')[0] === `${BASE}pandoc/pandoc.wasm.gz` ? new Response(gz.slice()) : new Response('', { status: 404 }))) as unknown as typeof fetch,
      },
    });
    const loadSpy = vi.spyOn(loader, 'load');
    const runner = new PandocRunner({ loader, createWorker: nodePandocWorker });
    const svc = createImportService({
      runner,
      wasm: {
        ready: async () => undefined,
        getImportFormatTable: () => JSON.parse(wasm.get_import_formats()),
        prepareImport: (n, s, h) => JSON.parse(wasm.prepare_import(n, s, h)),
        finishImport: (j, e, t, m, f) => JSON.parse(wasm.finish_import(j, e, t, m, f)),
        classifyImportFailure: (k, s, e) => JSON.parse(wasm.classify_import_failure(k, s, e)),
      },
      convertImage,
      sha256: async (b) => sha256(b),
      now: () => performance.now(),
      maxImageBytes: FILE_SIZE_LIMITS.MAX_FILE_SIZE,
    });
    return { svc, loadSpy };
  }

  const fixtures = importRecordingNames().filter((n) => n !== 'corrupt-docx');
  const shared = () => (shared.cached ??= make());
  shared.cached = undefined as ReturnType<typeof make> | undefined;

  it.each(fixtures)('%s imports to its expected.qmd and stores its media', async (name) => {
    const rec = loadImportRecording(name);
    const { svc } = shared();
    const file = new File([rec.source.slice()], `source.${extOf(name)}`);
    const out = await svc.importDocument(file, `${name}.qmd`);
    if (!out.ok) throw new Error(`${name}: ${JSON.stringify(out.diagnostics)}`);
    expect(out.qmd).toBe(expectedQmd(name));
    // One stored file per distinct image, and its bytes are the fixture's (the converter fails, so the originals).
    const fixtureBytes = new Map(rec.media.map((m) => [m.sha256, m.bytes]));
    expect(new Set(out.media.map((m) => sha256(m.bytes)))).toEqual(new Set(fixtureBytes.keys()));
    for (const m of out.media) {
      expect(m.projectPath).toMatch(new RegExp(`^${name}_media/${sha256(m.bytes).slice(0, 12)}\\.[a-z]+$`));
      expect(Buffer.from(m.bytes).equals(Buffer.from(fixtureBytes.get(sha256(m.bytes))!))).toBe(true);
    }
  });

  it('emf-docx with a failing converter stores the EMF and WMF as they are, and the report says so', async () => {
    const rec = loadImportRecording('emf-docx');
    const { svc } = make();
    const out = await svc.importDocument(new File([rec.source.slice()], 'source.docx'), 'emf-docx.qmd');
    if (!out.ok) throw new Error(JSON.stringify(out.diagnostics));
    expect(out.media.map((m) => m.projectPath.split('.').pop()).sort()).toEqual(['emf', 'wmf']);
    expect(out.diagnostics.filter((d) => 'code' in d && d.code === 'Q-24-9')).not.toHaveLength(0);
  });

  it('emf-docx with a converter that works links the PNGs, not the originals', async () => {
    const rec = loadImportRecording('emf-docx');
    const convertImage = vi.fn(async () => FAKE_PNG);
    const { svc } = make(convertImage);
    const out = await svc.importDocument(new File([rec.source.slice()], 'source.docx'), 'emf-docx.qmd');
    if (!out.ok) throw new Error(JSON.stringify(out.diagnostics));
    expect(convertImage).toHaveBeenCalledTimes(2);
    const png = `emf-docx_media/${sha256(FAKE_PNG).slice(0, 12)}.png`;
    // Both metafiles became the same PNG bytes, so they merge into one stored file.
    expect(out.media.map((m) => m.projectPath)).toEqual([png]);
    expect(out.qmd).toContain(png);
    expect(out.qmd).not.toMatch(/\.(emf|wmf)\)/);
    expect(out.diagnostics.filter((d) => 'code' in d && d.code === 'Q-24-10')).not.toHaveLength(0);
  });

  it('corrupt-docx fails as Q-24-3', async () => {
    const rec = loadImportRecording('corrupt-docx');
    const { svc } = make();
    const out: ImportOutcome = await svc.importDocument(new File([rec.source.slice()], 'source.docx'), 'corrupt.qmd');
    expect(out.ok).toBe(false);
    expect(out).toMatchObject({ diagnostics: [{ code: 'Q-24-3' }] });
  });

  it('refuses an over-cap file with Q-24-2, and an unknown type with Q-24-1, without loading pandoc', async () => {
    const { svc, loadSpy } = make();
    const table = await svc.getImportFormats();
    expect(table.maxSourceBytes).toBe(26214400);
    expect(table.formats.map((f) => f.id)).toEqual(['docx', 'odt', 'rtf', 'epub', 'pptx']);
    const big = new File([new Uint8Array(table.maxSourceBytes + 1)], 'big.docx');
    expect(await svc.validateImportSource(big)).toMatchObject([{ code: 'Q-24-2' }]);
    expect(await svc.importDocument(big, 'big.qmd')).toMatchObject({ ok: false, diagnostics: [{ code: 'Q-24-2' }] });
    const notes = new File(['# hi'], 'notes.md');
    expect(await svc.validateImportSource(notes)).toMatchObject([{ code: 'Q-24-1' }]);
    expect(await svc.importDocument(notes, 'notes.qmd')).toMatchObject({ ok: false, diagnostics: [{ code: 'Q-24-1' }] });
    expect(await svc.validateImportSource(new File([new Uint8Array(10)], 'ok.docx'))).toEqual([]);
    expect(loadSpy).not.toHaveBeenCalled();
  });
});
