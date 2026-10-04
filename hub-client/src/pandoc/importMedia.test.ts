import { describe, expect, it, vi } from 'vitest';
import type { HostDiagnostic } from '@quarto/pandoc-host';
import { buildMedia, type MediaDeps } from './importMedia';
import { FILE_SIZE_LIMITS } from '../services/resourceService';

const MAX = FILE_SIZE_LIMITS.MAX_FILE_SIZE;
const DIR = '/__q2_share__/import/media';
const file = (name: string, bytes: Uint8Array) => ({ path: `${DIR}/${name}`, bytes });
const bytes = (n: number, fill = 1) => new Uint8Array(n).fill(fill);

function deps(over: Partial<MediaDeps> = {}): MediaDeps {
  return {
    convertImage: async (b) => new Uint8Array([0x89, 0x50, 0x4e, 0x47, b.byteLength]),
    sha256: async (b) => `sha-${b.byteLength}-${b[0]}`,
    now: () => 0,
    maxImageBytes: MAX,
    convertTimeoutMs: 50,
    convertBudgetMs: 60_000,
    ...over,
  };
}

const run = async (files: ReturnType<typeof file>[], d: MediaDeps = deps(), warnings: HostDiagnostic[] = [], signal?: AbortSignal) => {
  const r = await buildMedia(files, warnings, d, signal);
  if (r.cancelled) throw new Error('cancelled');
  return r;
};

describe('buildMedia', () => {
  it('stores ordinary images with their hash and extension, in collected order', async () => {
    const r = await run([file('a.png', bytes(3)), file('b.JPG', bytes(4, 2))]);
    expect(r.manifest).toEqual([
      { pandoc_path: `${DIR}/a.png`, status: 'stored', sha256: 'sha-3-1', ext: 'png' },
      { pandoc_path: `${DIR}/b.JPG`, status: 'stored', sha256: 'sha-4-2', ext: 'jpg' },
    ]);
    expect([...r.stored.keys()]).toEqual([`${DIR}/a.png`, `${DIR}/b.JPG`]);
  });

  it('converts an EMF or WMF to PNG: the entry hashes the PNG, with ext png and converted_from', async () => {
    const convertImage = vi.fn(async (_b: Uint8Array, _f: 'emf' | 'wmf') => new Uint8Array([9, 9]));
    const r = await run([file('x.emf', bytes(100)), file('y.wmf', bytes(50))], deps({ convertImage }));
    expect(convertImage.mock.calls.map((c) => c[1])).toEqual(['emf', 'wmf']);
    expect(r.manifest).toEqual([
      { pandoc_path: `${DIR}/x.emf`, status: 'stored', sha256: 'sha-2-9', ext: 'png', converted_from: 'emf' },
      { pandoc_path: `${DIR}/y.wmf`, status: 'stored', sha256: 'sha-2-9', ext: 'png', converted_from: 'wmf' },
    ]);
    expect([...r.stored.get(`${DIR}/x.emf`)!.bytes]).toEqual([9, 9]);
  });

  it('keeps the original, flagged conversion_failed, when the converter throws', async () => {
    const r = await run([file('x.emf', bytes(100))], deps({ convertImage: async () => { throw new Error('bad header'); } }));
    expect(r.manifest).toEqual([{ pandoc_path: `${DIR}/x.emf`, status: 'stored', sha256: 'sha-100-1', ext: 'emf', conversion_failed: true }]);
    expect(r.stored.get(`${DIR}/x.emf`)!.bytes.byteLength).toBe(100);
  });

  it('keeps the original when the converter hangs past the per-image timeout, and goes on to the next image', async () => {
    let calls = 0;
    const convertImage = () => (++calls === 1 ? new Promise<Uint8Array>(() => undefined) : Promise.resolve(new Uint8Array([7])));
    const r = await run([file('x.emf', bytes(10)), file('y.emf', bytes(20))], deps({ convertImage }));
    expect(r.manifest[0]).toMatchObject({ ext: 'emf', conversion_failed: true });
    expect(r.manifest[1]).toMatchObject({ ext: 'png', converted_from: 'emf' });
  });

  it('keeps an original that fits when its PNG exceeds the limit', async () => {
    const r = await run([file('x.emf', bytes(10))], deps({ maxImageBytes: 100, convertImage: async () => bytes(101) }));
    expect(r.manifest).toEqual([{ pandoc_path: `${DIR}/x.emf`, status: 'stored', sha256: 'sha-10-1', ext: 'emf', conversion_failed: true }]);
  });

  it('skips an image when neither the original nor its PNG fits', async () => {
    const r = await run([file('x.emf', bytes(150))], deps({ maxImageBytes: 100, convertImage: async () => bytes(200) }));
    expect(r.manifest).toEqual([{ pandoc_path: `${DIR}/x.emf`, status: 'skipped', reason: 'too-large', size: 150 }]);
    expect(r.stored.size).toBe(0);
  });

  it('accepts a file of exactly 10 MB and skips one of 10 MB + 1 byte, dropping its bytes', async () => {
    const r = await run([file('exact.png', bytes(MAX)), file('over.png', bytes(MAX + 1))]);
    expect(r.manifest[0]).toMatchObject({ status: 'stored', ext: 'png' });
    expect(r.manifest[1]).toEqual({ pandoc_path: `${DIR}/over.png`, status: 'skipped', reason: 'too-large', size: MAX + 1 });
    expect([...r.stored.keys()]).toEqual([`${DIR}/exact.png`]);
  });

  it('appends a skipped entry for each host collect-limit warning, after the collected files', async () => {
    const warn = (path: string, size?: number): HostDiagnostic => ({ origin: 'host', kind: 'warning', code: 'collect-limit', message: 'dropped', path, size });
    const other: HostDiagnostic = { origin: 'host', kind: 'warning', code: 'pandoc-oom', message: 'unrelated' };
    const r = await run([file('a.png', bytes(3))], deps(), [warn(`${DIR}/big.bmp`, 30_000_000), other, warn(`${DIR}/huge.tif`)]);
    expect(r.manifest.map((m) => [m.status, m.pandoc_path])).toEqual([
      ['stored', `${DIR}/a.png`],
      ['skipped', `${DIR}/big.bmp`],
      ['skipped', `${DIR}/huge.tif`],
    ]);
    expect(r.manifest[1]).toMatchObject({ reason: 'too-large', size: 30_000_000 });
  });

  it('stops converting once the 60 s budget is spent: later EMFs keep their originals, flagged', async () => {
    let t = 0;
    const convertImage = vi.fn(async () => {
      t += 61_000;
      return new Uint8Array([1]);
    });
    const r = await run([file('a.emf', bytes(5)), file('b.emf', bytes(6))], deps({ convertImage, now: () => t }));
    expect(convertImage).toHaveBeenCalledTimes(1);
    expect(r.manifest[0]).toMatchObject({ ext: 'png', converted_from: 'emf' });
    expect(r.manifest[1]).toMatchObject({ ext: 'emf', conversion_failed: true });
  });

  it('is cancelled by an aborted signal before an image, and after a conversion', async () => {
    const before = new AbortController();
    before.abort();
    expect(await buildMedia([file('a.png', bytes(1))], [], deps(), before.signal)).toEqual({ cancelled: true });

    const during = new AbortController();
    const convertImage = async () => {
      during.abort();
      return new Uint8Array([1]);
    };
    expect(await buildMedia([file('a.emf', bytes(1))], [], deps({ convertImage }), during.signal)).toEqual({ cancelled: true });
  });
});
