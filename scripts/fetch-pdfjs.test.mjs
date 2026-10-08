import { mkdtempSync, readdirSync, readFileSync, existsSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { fetchPdfjsViewer } from './fetch-pandoc-wasm.mjs';

const repo = path.resolve(path.dirname(new URL(import.meta.url).pathname), '..');
const zip = path.join(repo, '.cache/pdfjs/pdfjs-6.3.289-dist.zip');

const files = (dir, rel = '') =>
  readdirSync(path.join(dir, rel)).flatMap((n) => {
    const r = rel ? `${rel}/${n}` : n;
    return statSync(path.join(dir, r)).isDirectory() ? files(dir, r) : [r];
  });

// Needs the cached release zip (fetched by `node scripts/fetch-pandoc-wasm.mjs`); skip without it.
describe.skipIf(!existsSync(zip))('fetchPdfjsViewer destinations', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'pdfjs-dests-'));
  afterAll(() => rmSync(root, { recursive: true, force: true }));
  const full = path.join(root, 'full');
  const pruned = path.join(root, 'pruned');

  it('writes a full and an en-US-only tree that differ only in locale directories', async () => {
    await fetchPdfjsViewer({
      fromZip: zip,
      dests: [
        { dir: full, pruneLocales: false },
        { dir: pruned, pruneLocales: true },
      ],
    });
    const fullFiles = files(full).filter((f) => f !== '.stamp');
    const prunedFiles = files(pruned).filter((f) => f !== '.stamp');
    const localeDirs = new Set(fullFiles.filter((f) => /^web\/locale\/[^/]+\//.test(f)).map((f) => f.split('/')[2]));
    expect(localeDirs.size).toBe(113); // 114 entries under web/locale with locale.json
    expect(new Set(prunedFiles.filter((f) => /^web\/locale\/[^/]+\//.test(f)).map((f) => f.split('/')[2]))).toEqual(
      new Set(['en-US']),
    );
    expect(prunedFiles).toContain('web/locale/locale.json');
    expect(prunedFiles).toContain('web/locale/en-US/viewer.ftl');
    const nonLocale = (fs) => fs.filter((f) => !/^web\/locale\/[^/]+\//.test(f));
    expect(nonLocale(prunedFiles)).toEqual(nonLocale(fullFiles));
    for (const f of prunedFiles) expect(readFileSync(path.join(pruned, f)).equals(readFileSync(path.join(full, f)))).toBe(true);
    expect(existsSync(path.join(full, '.stamp'))).toBe(true);
    expect(existsSync(path.join(pruned, '.stamp'))).toBe(true);
  });
});
