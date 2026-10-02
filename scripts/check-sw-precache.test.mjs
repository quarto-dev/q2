import { describe, expect, it } from 'vitest';
import { forbiddenInSw } from './check-sw-precache.mjs';
import { patchWorker } from './fetch-pandoc-wasm.mjs';

describe('forbiddenInSw', () => {
  it('flags a precached pdf.js viewer asset', () => {
    expect(forbiddenInSw('precacheAndRoute([{url:"pdfjs/web/viewer.html",revision:null}])')).toEqual(['pdfjs/']);
  });
  it('passes a manifest without it', () => {
    expect(forbiddenInSw('precacheAndRoute([{url:"assets/index-abc.js",revision:null}])')).toEqual([]);
  });
});

describe('patchWorker', () => {
  const getter = 'return shadow(this, "fingerprints", [hashOriginal.toHex(), hashModified?.toHex() ?? null]);';
  it('replaces the computed fingerprint with the constant', () => {
    const out = patchWorker(`a();\n${getter}\nb();`, 'abc');
    expect(out).toContain('["abc", null]');
    expect(out).not.toContain('hashOriginal.toHex()');
  });
  it('refuses a worker without exactly one fingerprints getter', () => {
    expect(() => patchWorker('nothing here')).toThrow(/not found exactly once/);
    expect(() => patchWorker(`${getter}${getter}`)).toThrow(/not found exactly once/);
  });
});
