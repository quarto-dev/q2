/**
 * With the worker backend installed, the package's existing API runs the
 * engine through the worker: same names, the VFS reads answered from the
 * main-thread mirror, render artifacts carried back into it. The
 * synchronous engine exports stay importable for in-process users
 * (hub-client and its WASM tests).
 */

import { afterEach, describe, expect, it } from 'vitest';
import * as runtime from '../index';
import { createWorkerBackend } from './engineClient';
import { serveEngine, type EngineApi } from './engineHost';

function fakeEngine(): EngineApi {
  const files = new Map<string, string>();
  return {
    initWasm: async () => {},
    vfsAddFile: (path, content) => (files.set(path, content), { success: true }),
    vfsAddBinaryFile: () => ({ success: true }),
    vfsRemoveFile: (path) => ({ success: files.delete(path) }),
    vfsClear: () => (files.clear(), { success: true }),
    vfsListFiles: () => ({ success: true, files: [...files.keys()] }),
    vfsReadFile: (path) =>
      files.has(path) ? { success: true, content: files.get(path) } : { success: false },
    vfsReadBinaryFile: () => ({ success: false }),
    renderPageForPreview: async (path) => {
      const source = files.get(path) ?? '';
      files.set('/.quarto/project-artifacts/styles.css', `/* ${source} */`);
      return { success: true, ast_json: '{}', renderedSource: source };
    },
    resolvePandocFormats: () =>
      ({ success: true, key: 'typst-pdf', class: 'download' }) as never,
    parseQmdContentSync: (text) => ({ success: true, ast: text.toUpperCase() }) as never,
    applyNodeEdit: (content) => `${content} edited`,
    regenerateNestedBuffers: () => ({}),
  };
}

afterEach(() => runtime.installEngineBackend(null));

describe('the worker backend behind the package API', () => {
  it('keeps the synchronous engine exports importable', () => {
    for (const name of ['initWasm', 'vfsAddFile', 'vfsClear', 'vfsReadFile', 'parseQmdContentSync', 'applyNodeEdit']) {
      expect(typeof (runtime as Record<string, unknown>)[name]).toBe('function');
    }
  });

  it('renders in the worker and mirrors writes and artifacts on the main thread', async () => {
    const channel = new MessageChannel();
    serveEngine(channel.port2, fakeEngine());
    runtime.installEngineBackend(createWorkerBackend(channel.port1));

    expect(runtime.isWasmReady()).toBe(false);
    await runtime.initWasm();
    expect(runtime.isWasmReady()).toBe(true);

    runtime.vfsAddFile('index.qmd', '# Hi');
    expect(runtime.vfsReadFile('index.qmd')).toEqual({ success: true, content: '# Hi' });

    const result = await runtime.renderPageForPreview('index.qmd');
    expect(result).toMatchObject({ success: true, renderedSource: '# Hi' });
    expect(runtime.vfsReadFile('/.quarto/project-artifacts/styles.css').content).toBe('/* # Hi */');

    await expect(runtime.resolvePandocFormatsAsync('index.qmd')).resolves.toMatchObject({
      key: 'typst-pdf',
    });
    await expect(runtime.parseQmdContentAsync('x')).resolves.toEqual({ success: true, ast: 'X' });
    await expect(runtime.applyNodeEditAsync('c', '', '', '')).resolves.toBe('c edited');
    // No engine in this realm: the synchronous helpers fail loudly.
    expect(() => runtime.parseQmdContentSync('x')).toThrow(/not initialized/i);
    channel.port1.close();
  });
});
