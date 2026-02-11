import { describe, it, expect } from 'vitest';
import { unzipSync, strFromU8 } from 'fflate';
import { exportProjectAsZip } from './export-zip.js';
import type { SyncClient } from './client.js';

/** Create a mock SyncClient with the given text and binary files. */
function mockClient(opts: {
  connected?: boolean;
  textFiles?: Record<string, string>;
  binaryFiles?: Record<string, { content: Uint8Array; mimeType: string }>;
}): SyncClient {
  const textFiles = opts.textFiles ?? {};
  const binaryFiles = opts.binaryFiles ?? {};
  const allPaths = [
    ...Object.keys(textFiles),
    ...Object.keys(binaryFiles),
  ];

  return {
    isConnected: () => opts.connected ?? true,
    getFilePaths: () => allPaths,
    isFileBinary: (path: string) => path in binaryFiles,
    getFileContent: (path: string) => textFiles[path] ?? null,
    getBinaryFileContent: (path: string) => binaryFiles[path] ?? null,
  } as unknown as SyncClient;
}

describe('exportProjectAsZip', () => {
  it('throws when client is not connected', () => {
    const client = mockClient({ connected: false });
    expect(() => exportProjectAsZip(client)).toThrow(
      'SyncClient is not connected',
    );
  });

  it('produces a valid ZIP for an empty project', () => {
    const client = mockClient({});
    const zip = exportProjectAsZip(client);

    expect(zip).toBeInstanceOf(Uint8Array);
    expect(zip.length).toBeGreaterThan(0);

    const entries = unzipSync(zip);
    expect(Object.keys(entries)).toHaveLength(0);
  });

  it('includes text files encoded as UTF-8', () => {
    const client = mockClient({
      textFiles: {
        'index.qmd': '# Hello\n\nThis is a test.',
        'styles.css': 'body { color: red; }',
      },
    });

    const zip = exportProjectAsZip(client);
    const entries = unzipSync(zip);

    expect(Object.keys(entries)).toHaveLength(2);
    expect(strFromU8(entries['index.qmd'])).toBe('# Hello\n\nThis is a test.');
    expect(strFromU8(entries['styles.css'])).toBe('body { color: red; }');
  });

  it('includes binary files as raw bytes', () => {
    const pngBytes = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a]);
    const client = mockClient({
      binaryFiles: {
        'image.png': { content: pngBytes, mimeType: 'image/png' },
      },
    });

    const zip = exportProjectAsZip(client);
    const entries = unzipSync(zip);

    expect(Object.keys(entries)).toHaveLength(1);
    expect(entries['image.png']).toEqual(pngBytes);
  });

  it('handles a mix of text and binary files', () => {
    const gifBytes = new Uint8Array([0x47, 0x49, 0x46, 0x38]);
    const client = mockClient({
      textFiles: {
        'index.qmd': '---\ntitle: Test\n---',
        'src/utils.ts': 'export const x = 1;',
      },
      binaryFiles: {
        'images/logo.gif': { content: gifBytes, mimeType: 'image/gif' },
      },
    });

    const zip = exportProjectAsZip(client);
    const entries = unzipSync(zip);

    expect(Object.keys(entries)).toHaveLength(3);
    expect(strFromU8(entries['index.qmd'])).toBe('---\ntitle: Test\n---');
    expect(strFromU8(entries['src/utils.ts'])).toBe('export const x = 1;');
    expect(entries['images/logo.gif']).toEqual(gifBytes);
  });

  it('preserves nested directory paths in the ZIP', () => {
    const client = mockClient({
      textFiles: {
        'a/b/c/deep.txt': 'deep content',
      },
    });

    const zip = exportProjectAsZip(client);
    const entries = unzipSync(zip);

    expect(entries['a/b/c/deep.txt']).toBeDefined();
    expect(strFromU8(entries['a/b/c/deep.txt'])).toBe('deep content');
  });

  it('handles text files with Unicode content', () => {
    const client = mockClient({
      textFiles: {
        'unicode.qmd': 'Héllo wörld! 日本語テスト 🎉',
      },
    });

    const zip = exportProjectAsZip(client);
    const entries = unzipSync(zip);

    expect(strFromU8(entries['unicode.qmd'])).toBe(
      'Héllo wörld! 日本語テスト 🎉',
    );
  });

  it('skips files where getFileContent returns null', () => {
    // Simulate a file that exists in the path list but has no content
    const client = {
      isConnected: () => true,
      getFilePaths: () => ['exists.qmd', 'ghost.qmd'],
      isFileBinary: () => false,
      getFileContent: (path: string) =>
        path === 'exists.qmd' ? 'content' : null,
      getBinaryFileContent: () => null,
    } as unknown as SyncClient;

    const zip = exportProjectAsZip(client);
    const entries = unzipSync(zip);

    expect(Object.keys(entries)).toHaveLength(1);
    expect(strFromU8(entries['exists.qmd'])).toBe('content');
  });
});
