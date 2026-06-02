import { describe, it, expect } from 'vitest';
import { zipSync, strToU8, strFromU8 } from 'fflate';
import { parseProjectZip } from './import-zip.js';

/** Decode a base64 string (as produced for binary entries) back to bytes. */
function fromBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** Build a ZIP from a map of path -> bytes. */
function makeZip(files: Record<string, Uint8Array>): Uint8Array {
  return zipSync(files, { level: 0 });
}

describe('parseProjectZip', () => {
  it('parses text files as UTF-8 strings', () => {
    const zip = makeZip({
      'index.qmd': strToU8('# Hello\n\nThis is a test.'),
      'styles.css': strToU8('body { color: red; }'),
    });

    const files = parseProjectZip(zip);
    const byPath = Object.fromEntries(files.map(f => [f.path, f]));

    expect(files).toHaveLength(2);
    expect(byPath['index.qmd'].contentType).toBe('text');
    expect(byPath['index.qmd'].content).toBe('# Hello\n\nThis is a test.');
    expect(byPath['styles.css'].contentType).toBe('text');
    expect(byPath['styles.css'].content).toBe('body { color: red; }');
  });

  it('parses binary files as base64 with an inferred MIME type', () => {
    const pngBytes = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    const zip = makeZip({ 'image.png': pngBytes });

    const files = parseProjectZip(zip);
    expect(files).toHaveLength(1);
    expect(files[0].path).toBe('image.png');
    expect(files[0].contentType).toBe('binary');
    expect(files[0].mimeType).toBe('image/png');
    expect(fromBase64(files[0].content)).toEqual(pngBytes);
  });

  it('preserves Unicode text content', () => {
    const zip = makeZip({
      'unicode.qmd': strToU8('Héllo wörld! 日本語テスト 🎉'),
    });

    const files = parseProjectZip(zip);
    expect(files[0].content).toBe('Héllo wörld! 日本語テスト 🎉');
    expect(files[0].contentType).toBe('text');
  });

  it('preserves nested directory paths', () => {
    const zip = makeZip({
      'a/b/c/deep.txt': strToU8('deep content'),
    });

    const files = parseProjectZip(zip);
    expect(files).toHaveLength(1);
    expect(files[0].path).toBe('a/b/c/deep.txt');
    expect(files[0].content).toBe('deep content');
  });

  it('handles a mix of text and binary files', () => {
    const gifBytes = new Uint8Array([0x47, 0x49, 0x46, 0x38, 0x39, 0x61]);
    const zip = makeZip({
      'index.qmd': strToU8('---\ntitle: Test\n---'),
      'src/utils.ts': strToU8('export const x = 1;'),
      'images/logo.gif': gifBytes,
    });

    const files = parseProjectZip(zip);
    const byPath = Object.fromEntries(files.map(f => [f.path, f]));

    expect(files).toHaveLength(3);
    expect(byPath['index.qmd'].contentType).toBe('text');
    expect(byPath['src/utils.ts'].contentType).toBe('text');
    expect(byPath['images/logo.gif'].contentType).toBe('binary');
    expect(fromBase64(byPath['images/logo.gif'].content)).toEqual(gifBytes);
  });

  describe('top-level directory stripping', () => {
    it('strips a single common leading directory (GitHub-style download)', () => {
      const zip = makeZip({
        'my-repo-main/index.qmd': strToU8('# Home'),
        'my-repo-main/about.qmd': strToU8('# About'),
        'my-repo-main/images/logo.png': new Uint8Array([0x89, 0x50, 0x4e, 0x47]),
      });

      const files = parseProjectZip(zip);
      const paths = files.map(f => f.path).sort();
      expect(paths).toEqual(['about.qmd', 'images/logo.png', 'index.qmd']);
    });

    it('does not strip when entries do not all share one leading segment', () => {
      const zip = makeZip({
        'index.qmd': strToU8('# Home'),
        'images/logo.png': new Uint8Array([0x89, 0x50, 0x4e, 0x47]),
      });

      const files = parseProjectZip(zip);
      const paths = files.map(f => f.path).sort();
      expect(paths).toEqual(['images/logo.png', 'index.qmd']);
    });

    it('does not strip a common prefix when only one file is present', () => {
      // A single nested file is meaningful structure, not a wrapper dir.
      const zip = makeZip({
        'docs/index.qmd': strToU8('# Home'),
      });

      const files = parseProjectZip(zip);
      expect(files.map(f => f.path)).toEqual(['docs/index.qmd']);
    });
  });

  describe('junk filtering', () => {
    it('skips directory entries', () => {
      // fflate represents directory entries as zero-length entries with a
      // trailing slash; construct one explicitly.
      const zip = zipSync(
        {
          'dir/': new Uint8Array(0),
          'dir/file.qmd': strToU8('content'),
        },
        { level: 0 },
      );

      const files = parseProjectZip(zip);
      expect(files.map(f => f.path)).toEqual(['dir/file.qmd']);
    });

    it('skips __MACOSX and .DS_Store junk', () => {
      const zip = makeZip({
        'index.qmd': strToU8('# Home'),
        '__MACOSX/._index.qmd': new Uint8Array([0, 1, 2]),
        '.DS_Store': new Uint8Array([0, 1, 2]),
        'sub/.DS_Store': new Uint8Array([0, 1, 2]),
      });

      const files = parseProjectZip(zip);
      expect(files.map(f => f.path)).toEqual(['index.qmd']);
    });

    it('skips .git internal files', () => {
      const zip = makeZip({
        'index.qmd': strToU8('# Home'),
        '.git/config': strToU8('[core]'),
        '.git/objects/ab/cdef': new Uint8Array([1, 2, 3]),
      });

      const files = parseProjectZip(zip);
      expect(files.map(f => f.path)).toEqual(['index.qmd']);
    });
  });

  describe('binary-vs-text classification of unknown extensions', () => {
    it('treats an unknown extension with valid UTF-8 as text', () => {
      const zip = makeZip({
        'data.unknownext': strToU8('plain text payload'),
      });

      const files = parseProjectZip(zip);
      expect(files[0].contentType).toBe('text');
      expect(files[0].content).toBe('plain text payload');
    });

    it('treats an unknown extension containing NUL bytes as binary', () => {
      const bytes = new Uint8Array([0x00, 0x01, 0x02, 0x00, 0xff]);
      const zip = makeZip({ 'data.unknownext': bytes });

      const files = parseProjectZip(zip);
      expect(files[0].contentType).toBe('binary');
      expect(fromBase64(files[0].content)).toEqual(bytes);
    });

    it('treats an unknown extension with invalid UTF-8 as binary', () => {
      // 0xC3 starts a 2-byte sequence but 0x28 is not a valid continuation.
      const bytes = new Uint8Array([0x41, 0xc3, 0x28, 0x42]);
      const zip = makeZip({ 'data.unknownext': bytes });

      const files = parseProjectZip(zip);
      expect(files[0].contentType).toBe('binary');
      expect(fromBase64(files[0].content)).toEqual(bytes);
    });
  });

  describe('path-safety (zip-slip)', () => {
    it('rejects entries that escape via ..', () => {
      const zip = makeZip({
        '../evil.qmd': strToU8('pwned'),
      });
      expect(() => parseProjectZip(zip)).toThrow(/unsafe path/i);
    });

    it('rejects absolute-path entries', () => {
      const zip = makeZip({
        '/etc/passwd': strToU8('root:x:0:0'),
      });
      expect(() => parseProjectZip(zip)).toThrow(/unsafe path/i);
    });
  });

  describe('error handling', () => {
    it('throws on an empty archive', () => {
      const zip = makeZip({});
      expect(() => parseProjectZip(zip)).toThrow(/no files/i);
    });

    it('throws on an archive with only junk', () => {
      const zip = makeZip({
        '.DS_Store': new Uint8Array([1, 2, 3]),
        '__MACOSX/._x': new Uint8Array([1, 2, 3]),
      });
      expect(() => parseProjectZip(zip)).toThrow(/no files/i);
    });

    it('throws on corrupt ZIP bytes', () => {
      const garbage = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]);
      expect(() => parseProjectZip(garbage)).toThrow();
    });
  });

  it('round-trips a hub-client-style export (zipSync -> parseProjectZip)', () => {
    // Mirror what exportProjectAsZip produces: text via strToU8, binary raw.
    const original: Record<string, Uint8Array> = {
      'index.qmd': strToU8('# Hello\n\nThis is a test.'),
      'src/utils.ts': strToU8('export const x = 1;'),
      'images/logo.gif': new Uint8Array([0x47, 0x49, 0x46, 0x38]),
      'unicode.qmd': strToU8('日本語テスト 🎉'),
    };
    const zip = zipSync(original, { level: 6 });

    const files = parseProjectZip(zip);
    expect(files).toHaveLength(4);

    for (const f of files) {
      const expected = original[f.path];
      expect(expected, `unexpected path ${f.path}`).toBeDefined();
      const actual =
        f.contentType === 'binary' ? fromBase64(f.content) : strToU8(f.content);
      expect(actual).toEqual(expected);
    }
  });
});
