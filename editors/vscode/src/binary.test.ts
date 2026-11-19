import { describe, expect, it } from 'vitest';
import * as path from 'node:path';

import { findQ2, type BinaryLookup } from './binary';

function lookup(files: string[], over: Partial<BinaryLookup> = {}): BinaryLookup {
  const set = new Set(files);
  return {
    configured: '',
    pathEnv: '',
    pathExt: undefined,
    platform: 'linux',
    home: '/home/u',
    isFile: (p) => set.has(p),
    ...over,
  };
}

describe('findQ2', () => {
  it('prefers the configured path', () => {
    const r = findQ2(lookup(['/opt/q2', '/usr/bin/q2'], { configured: '/opt/q2', pathEnv: '/usr/bin' }));
    expect(r).toEqual({ found: true, path: '/opt/q2', source: 'setting' });
  });

  it('expands ~ and resolves relative settings against the workspace', () => {
    expect(findQ2(lookup([path.join('/home/u', 'bin/q2')], { configured: '~/bin/q2' }))).toMatchObject({
      found: true,
      source: 'setting',
    });
    expect(
      findQ2(lookup([path.join('/ws', 'target/debug/q2')], { configured: 'target/debug/q2', workspaceRoot: '/ws' })),
    ).toMatchObject({ found: true, path: path.join('/ws', 'target/debug/q2') });
  });

  it('accepts a configured path without .exe on Windows', () => {
    const exe = path.join('C:\\repo', 'q2.exe');
    const r = findQ2(lookup([exe], { platform: 'win32', configured: path.join('C:\\repo', 'q2') }));
    expect(r).toMatchObject({ found: true, path: exe });
  });

  it('reports a configured path that does not exist instead of falling back', () => {
    const r = findQ2(lookup(['/usr/bin/q2'], { configured: '/nope/q2', pathEnv: '/usr/bin' }));
    expect(r.found).toBe(false);
  });

  it('walks PATH in order', () => {
    const r = findQ2(lookup(['/b/q2', '/c/q2'], { pathEnv: '/a::/b:/c' }));
    expect(r).toEqual({ found: true, path: path.join('/b', 'q2'), source: 'path' });
  });

  it('uses PATHEXT on Windows', () => {
    const exe = path.join('C:\\tools', 'q2.exe');
    const r = findQ2(lookup([exe], { platform: 'win32', pathEnv: 'C:\\tools', pathExt: '.COM;.EXE' }));
    expect(r).toMatchObject({ found: true, path: exe, source: 'path' });
  });

  it('falls back to ~/.local/bin', () => {
    const p = path.join('/home/u', '.local', 'bin', 'q2');
    expect(findQ2(lookup([p]))).toEqual({ found: true, path: p, source: 'install-dir' });
  });

  it('says so when nothing is found', () => {
    expect(findQ2(lookup([])).found).toBe(false);
  });
});
