import { describe, expect, it } from 'vitest';
import { createXhrMissHandler, type SyncRequest } from './missShim';

/** A fake synchronous transport over a table of `route?path` answers. */
function transport(table: Record<string, { status: number; body?: unknown }>) {
  const calls: string[] = [];
  const request: SyncRequest = (url) => {
    const key = url.replace('http://h/api/preview/', '');
    calls.push(decodeURIComponent(key));
    const hit = table[decodeURIComponent(key)] ?? { status: 404 };
    return {
      status: hit.status,
      body:
        hit.body instanceof Uint8Array
          ? hit.body
          : new TextEncoder().encode(hit.body === undefined ? '' : JSON.stringify(hit.body)),
    };
  };
  return { request, calls };
}

describe('createXhrMissHandler', () => {
  it('answers the three arms from the miss routes', () => {
    const { request, calls } = transport({
      'file?path=refs.bib': { status: 200, body: new Uint8Array([64, 97]) },
      'stat?path=partials': { status: 200, body: { exists: true, isFile: false, isDir: true } },
      'list?path=partials': { status: 200, body: { entries: [{ name: '_vars.scss', kind: 'file' }] } },
    });
    const pulled: Array<[string, Uint8Array]> = [];
    const handler = createXhrMissHandler('http://h/api/preview/', {
      request,
      onPull: (path, bytes) => pulled.push([path, bytes]),
    });

    expect(handler.read('refs.bib')).toEqual(new Uint8Array([64, 97]));
    expect(pulled).toEqual([['refs.bib', new Uint8Array([64, 97])]]);
    expect(handler.stat('partials')).toEqual({ exists: true, isFile: false, isDir: true });
    expect(handler.list('partials')).toEqual([{ name: '_vars.scss', kind: 'file' }]);
    expect(calls).toEqual(['file?path=refs.bib', 'stat?path=partials', 'list?path=partials']);
  });

  it('treats 404, 403 and transport failures as absent, and counts per arm and status', () => {
    const { request } = transport({ 'file?path=.env': { status: 403 } });
    const failing: SyncRequest = () => {
      throw new Error('network down');
    };
    const handler = createXhrMissHandler('http://h/api/preview/', { request });
    expect(handler.read('missing.scss')).toBeNull();
    expect(handler.read('.env')).toBeNull();
    expect(handler.stat('nope')).toEqual({ exists: false, isFile: false, isDir: false });
    expect(handler.list('nope')).toBeNull();
    expect(handler.counts()).toEqual({
      'read.404': 1,
      'read.403': 1,
      'stat.404': 1,
      'list.404': 1,
    });

    const offline = createXhrMissHandler('http://h/api/preview/', { request: failing });
    expect(offline.read('a.qmd')).toBeNull();
    expect(offline.counts()).toEqual({ 'read.error': 1 });
  });
});
