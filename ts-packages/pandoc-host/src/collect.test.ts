import { Directory, File } from '@bjorn3/browser_wasi_shim';
import { describe, expect, it } from 'vitest';
import { DEFAULT_LIMITS } from './limits.ts';
import { collectFiles, type Tree } from './shared.ts';
import { prepareInputsForPost } from './transfer.ts';

const dir = (entries: Record<string, Directory | File>) => new Directory(new Map(Object.entries(entries)));
const file = (n: number) => new File(new Uint8Array(n).fill(7));
const treeOf = (entries: Record<string, Directory | File>): Tree => new Map(Object.entries(entries));
const limits = { ...DEFAULT_LIMITS, collected_file_bytes: 10, collected_total_bytes: 25 };
const paths = (r: ReturnType<typeof collectFiles>) => r.collected.map((f) => f.path);

describe('collectFiles', () => {
  const tree = treeOf({
    __q2_share__: dir({
      import: dir({
        'out.json': file(1),
        media: dir({ 'b.png': file(2), 'a.png': file(3), sub: dir({ 'c.emf': file(4) }) }),
      }),
    }),
  });

  it('returns every regular file under the directory, with absolute paths, sorted', () => {
    const r = collectFiles(tree, ['/__q2_share__/import/media'], DEFAULT_LIMITS);
    expect(paths(r)).toEqual(['/__q2_share__/import/media/a.png', '/__q2_share__/import/media/b.png', '/__q2_share__/import/media/sub/c.emf']);
    expect(r.diagnostics).toEqual([]);
    expect(r.collected[0].bytes).toHaveLength(3);
  });

  it('a missing directory, or one that is a file, yields nothing without a diagnostic', () => {
    expect(collectFiles(tree, ['/__q2_share__/import/nope'], DEFAULT_LIMITS)).toEqual({ collected: [], diagnostics: [] });
    expect(collectFiles(tree, ['/__q2_share__/import/out.json'], DEFAULT_LIMITS)).toEqual({ collected: [], diagnostics: [] });
    expect(collectFiles(tree, [], DEFAULT_LIMITS)).toEqual({ collected: [], diagnostics: [] });
  });

  it('overlapping directories list a file once, sorted across directories', () => {
    const r = collectFiles(tree, ['/__q2_share__/import/media/sub', '/__q2_share__/import/media'], DEFAULT_LIMITS);
    expect(paths(r)).toHaveLength(3);
    expect(paths(r)).toEqual([...paths(r)].sort());
  });

  it('a file over the per-file limit is dropped with a warning naming its path and size; others stay', () => {
    const t = treeOf({ m: dir({ 'a.bin': file(11), 'b.bin': file(10), 'c.bin': file(1) }) });
    const r = collectFiles(t, ['/m'], limits);
    expect(paths(r)).toEqual(['/m/b.bin', '/m/c.bin']);
    expect(r.diagnostics).toEqual([
      { origin: 'host', kind: 'warning', code: 'collect-limit', message: expect.stringContaining('per-file limit'), path: '/m/a.bin', size: 11 },
    ]);
  });

  it('in path order, the file that would pass the total is dropped and later files are still checked', () => {
    // 10 + 10 = 20 kept; c (10) would make 30 > 25 and is dropped; d (5) still fits (25); e (1) does not.
    const t = treeOf({ m: dir({ 'a.bin': file(10), 'b.bin': file(10), 'c.bin': file(10), 'd.bin': file(5), 'e.bin': file(1) }) });
    const r = collectFiles(t, ['/m'], limits);
    expect(paths(r)).toEqual(['/m/a.bin', '/m/b.bin', '/m/d.bin']);
    expect(r.diagnostics.map((d) => [d.path, d.size, d.code])).toEqual([
      ['/m/c.bin', 10, 'collect-limit'],
      ['/m/e.bin', 1, 'collect-limit'],
    ]);
    expect(r.diagnostics[0].message).toContain('limit is 25');
  });

  it('a file at the per-file limit and a total exactly at the limit are kept', () => {
    const t = treeOf({ m: dir({ 'a.bin': file(10), 'b.bin': file(10), 'c.bin': file(5) }) });
    const r = collectFiles(t, ['/m'], limits);
    expect(paths(r)).toHaveLength(3);
    expect(r.diagnostics).toEqual([]);
  });
});

describe('prepareInputsForPost', () => {
  it('transfers whole buffers once, copies sub-views, and leaves the keys alone', () => {
    const whole = new Uint8Array([1, 2, 3]);
    const big = new Uint8Array(10).fill(9);
    const view = big.subarray(2, 5);
    const { value, transfer } = prepareInputsForPost({ '/a': whole, '/b': view, '/c': whole });
    expect(Object.keys(value)).toEqual(['/a', '/b', '/c']);
    expect(value['/a']).toBe(whole);
    expect(value['/b']).not.toBe(view);
    expect(value['/b'].buffer).not.toBe(big.buffer);
    expect([...value['/b']]).toEqual([9, 9, 9]);
    expect(transfer).toHaveLength(2); // `whole` once, the copied view's fresh buffer
    expect(transfer).not.toContain(big.buffer);
  });
});
