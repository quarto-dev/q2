/**
 * @vitest-environment jsdom
 *
 * Tests for the dev-only console debug API (`window.quartoDebug`).
 *
 * Mocks both `./automergeSync` and `./wasmRenderer` so the tests
 * exercise the API surface and the routing through to those
 * modules without booting WASM or Automerge. Uses jsdom because
 * the install path mutates `window`.
 *
 * Tracking: bd-2rv8.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import type { FileEntry } from '@quarto/quarto-automerge-schema';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';

const automergeSyncMocks = vi.hoisted(() => ({
  isFileBinary: vi.fn<(path: string) => boolean>(),
  getFileContent: vi.fn<(path: string) => string | null>(),
  getBinaryFileContent: vi.fn<(path: string) => { content: Uint8Array; mimeType: string } | null>(),
  getFilePaths: vi.fn<() => string[]>(),
  updateFileContent: vi.fn<(path: string, content: string) => void>(),
  createFile: vi.fn<(path: string, content: string) => Promise<void>>(),
  createBinaryFile: vi.fn<(path: string, content: Uint8Array, mimeType: string) => Promise<unknown>>(),
  deleteFile: vi.fn<(path: string) => void>(),
}));

const wasmRendererMocks = vi.hoisted(() => ({
  renderToHtml: vi.fn<(opts: { documentPath: string }) => Promise<{ html: string; success: boolean }>>(),
  setRenderListener: vi.fn(),
  vfsListFiles: vi.fn<() => { success: boolean; files?: string[] }>(),
  vfsReadBinaryFile: vi.fn<(path: string) => { success: boolean; content?: string }>(),
}));

vi.mock('./automergeSync', () => automergeSyncMocks);
vi.mock('./wasmRenderer', () => wasmRendererMocks);

import {
  installDebugApi,
  uninstallDebugApi,
  _getInstalledApiForTesting,
  type DebugApiContext,
} from './debugApi';

const sampleProject: ProjectEntry = {
  id: 'proj-1',
  indexDocId: 'automerge:abc123',
  syncServer: 'wss://sync.example.com',
  description: 'Sample',
  createdAt: '2026-05-01T00:00:00Z',
  lastAccessed: '2026-05-01T00:00:00Z',
};

const sampleFiles: FileEntry[] = [
  { path: 'index.qmd', docId: 'automerge:f1' },
  { path: 'about.qmd', docId: 'automerge:f2' },
  { path: 'logo.png', docId: 'automerge:f3' },
];

function makeContext(overrides: Partial<DebugApiContext> = {}): DebugApiContext {
  return {
    getProject: () => sampleProject,
    getFiles: () => sampleFiles,
    getActiveFile: () => 'index.qmd',
    setActiveFile: vi.fn(),
    ...overrides,
  };
}

describe('debugApi', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    automergeSyncMocks.isFileBinary.mockImplementation((p: string) =>
      p.endsWith('.png'),
    );
    automergeSyncMocks.getFileContent.mockImplementation((p: string) =>
      p === 'index.qmd' ? 'hello' : p === 'about.qmd' ? 'about body' : null,
    );
    automergeSyncMocks.getBinaryFileContent.mockImplementation((p: string) =>
      p === 'logo.png'
        ? { content: new Uint8Array([1, 2, 3]), mimeType: 'image/png' }
        : null,
    );
    automergeSyncMocks.createFile.mockResolvedValue(undefined);
    automergeSyncMocks.createBinaryFile.mockResolvedValue({});
    wasmRendererMocks.renderToHtml.mockResolvedValue({
      html: '<p>ok</p>',
      success: true,
    });
    wasmRendererMocks.vfsListFiles.mockReturnValue({
      success: true,
      files: ['/project/index.qmd', '/.quarto/project-artifacts/styles.css'],
    });
    wasmRendererMocks.vfsReadBinaryFile.mockReturnValue({
      success: true,
      // base64 of "hi"
      content: 'aGk=',
    });
  });

  afterEach(() => {
    uninstallDebugApi();
  });

  describe('install/uninstall', () => {
    it('exposes window.quartoDebug after install and removes it on uninstall', () => {
      expect((window as unknown as Record<string, unknown>).quartoDebug).toBeUndefined();

      const uninstall = installDebugApi(makeContext());
      const api = (window as unknown as Record<string, unknown>).quartoDebug;
      expect(api).toBeDefined();
      expect(_getInstalledApiForTesting()).toBe(api);

      uninstall();
      expect((window as unknown as Record<string, unknown>).quartoDebug).toBeUndefined();
      expect(_getInstalledApiForTesting()).toBeNull();
    });

    it('installs a render listener and clears it on uninstall', () => {
      installDebugApi(makeContext());
      expect(wasmRendererMocks.setRenderListener).toHaveBeenCalledTimes(1);
      expect(wasmRendererMocks.setRenderListener).toHaveBeenLastCalledWith(
        expect.any(Function),
      );

      uninstallDebugApi();
      expect(wasmRendererMocks.setRenderListener).toHaveBeenLastCalledWith(null);
    });

    it('replacing the install tears the previous one down first', () => {
      installDebugApi(makeContext());
      installDebugApi(makeContext());
      // setRenderListener calls in order: install (fn), uninstall (null),
      // re-install (fn) — three total.
      expect(wasmRendererMocks.setRenderListener).toHaveBeenCalledTimes(3);
      expect(wasmRendererMocks.setRenderListener.mock.calls[1][0]).toBeNull();
    });
  });

  describe('reads', () => {
    it('project() returns the live project info', () => {
      installDebugApi(makeContext());
      const api = _getInstalledApiForTesting()!;
      expect(api.project()).toEqual({
        id: 'proj-1',
        description: 'Sample',
        indexDocId: 'automerge:abc123',
        syncServer: 'wss://sync.example.com',
      });
    });

    it('project() returns null when no project is loaded', () => {
      installDebugApi(makeContext({ getProject: () => null }));
      expect(_getInstalledApiForTesting()!.project()).toBeNull();
    });

    it('listFiles() reflects the current FileEntry list', () => {
      installDebugApi(makeContext());
      expect(_getInstalledApiForTesting()!.listFiles()).toEqual([
        'index.qmd',
        'about.qmd',
        'logo.png',
      ]);
    });

    it('readFile() returns text content for text files', () => {
      installDebugApi(makeContext());
      expect(_getInstalledApiForTesting()!.readFile('index.qmd')).toBe('hello');
    });

    it('readFile() returns Uint8Array for binary files', () => {
      installDebugApi(makeContext());
      const result = _getInstalledApiForTesting()!.readFile('logo.png');
      expect(result).toBeInstanceOf(Uint8Array);
      expect(Array.from(result as Uint8Array)).toEqual([1, 2, 3]);
    });

    it('readFile() returns null for unknown paths', () => {
      installDebugApi(makeContext());
      expect(_getInstalledApiForTesting()!.readFile('nope.qmd')).toBeNull();
    });
  });

  describe('writeFile', () => {
    it('updates an existing text file via updateFileContent', async () => {
      installDebugApi(makeContext());
      await _getInstalledApiForTesting()!.writeFile('index.qmd', 'new body');
      expect(automergeSyncMocks.updateFileContent).toHaveBeenCalledWith(
        'index.qmd',
        'new body',
      );
      expect(automergeSyncMocks.createFile).not.toHaveBeenCalled();
    });

    it('creates a new text file via createFile when absent', async () => {
      installDebugApi(makeContext());
      await _getInstalledApiForTesting()!.writeFile('new.qmd', 'fresh body');
      expect(automergeSyncMocks.createFile).toHaveBeenCalledWith(
        'new.qmd',
        'fresh body',
      );
      expect(automergeSyncMocks.updateFileContent).not.toHaveBeenCalled();
    });

    it('rejects writing a string into an existing binary file', async () => {
      installDebugApi(makeContext());
      await expect(
        _getInstalledApiForTesting()!.writeFile('logo.png', 'oops'),
      ).rejects.toThrow(/binary file/);
    });

    it('overwrites an existing binary file via delete + create to preserve the path', async () => {
      installDebugApi(makeContext());
      const bytes = new Uint8Array([9, 8, 7]);
      await _getInstalledApiForTesting()!.writeFile('logo.png', bytes, {
        mimeType: 'image/png',
      });
      expect(automergeSyncMocks.deleteFile).toHaveBeenCalledWith('logo.png');
      expect(automergeSyncMocks.createBinaryFile).toHaveBeenCalledWith(
        'logo.png',
        bytes,
        'image/png',
      );
      // Ordering: deleteFile must have been called before createBinaryFile so
      // the path is freed before the new content-addressed write.
      const deleteOrder = automergeSyncMocks.deleteFile.mock.invocationCallOrder[0];
      const createOrder = automergeSyncMocks.createBinaryFile.mock.invocationCallOrder[0];
      expect(deleteOrder).toBeLessThan(createOrder);
    });

    it('infers MIME type from extension when not provided', async () => {
      installDebugApi(makeContext());
      const bytes = new Uint8Array([1]);
      await _getInstalledApiForTesting()!.writeFile('new.png', bytes);
      const [, , mime] = automergeSyncMocks.createBinaryFile.mock.calls[0];
      expect(mime).toBe('image/png');
    });

    it('does not delete when creating a brand-new binary file', async () => {
      installDebugApi(makeContext());
      await _getInstalledApiForTesting()!.writeFile('new.png', new Uint8Array([1]));
      expect(automergeSyncMocks.deleteFile).not.toHaveBeenCalled();
      expect(automergeSyncMocks.createBinaryFile).toHaveBeenCalledTimes(1);
    });
  });

  describe('rerender / lastRenderResponse', () => {
    it('rerender calls renderToHtml for the active page and returns a snapshot', async () => {
      installDebugApi(makeContext());
      const api = _getInstalledApiForTesting()!;
      const snap = await api.rerender();
      expect(wasmRendererMocks.renderToHtml).toHaveBeenCalledWith({
        documentPath: 'index.qmd',
      });
      expect(snap.documentPath).toBe('index.qmd');
      expect(snap.result.html).toBe('<p>ok</p>');
      expect(typeof snap.at).toBe('number');
    });

    it('rerender throws when no active file', async () => {
      installDebugApi(makeContext({ getActiveFile: () => null }));
      await expect(_getInstalledApiForTesting()!.rerender()).rejects.toThrow(
        /no active file/,
      );
    });

    it('lastRenderResponse() reflects the most recent render', async () => {
      installDebugApi(makeContext());
      const api = _getInstalledApiForTesting()!;
      expect(api.lastRenderResponse()).toBeNull();
      await api.rerender();
      const last = api.lastRenderResponse();
      expect(last?.documentPath).toBe('index.qmd');
      expect(last?.result.html).toBe('<p>ok</p>');
    });

    it('lastRenderResponse() also captures editor-driven renders via the listener', () => {
      installDebugApi(makeContext());
      // Pull the listener out of the mock and invoke it as the renderer would.
      const listener = wasmRendererMocks.setRenderListener.mock.calls[0][0] as
        | ((result: unknown, options: unknown) => void)
        | null;
      expect(listener).toBeTypeOf('function');
      listener?.(
        { html: '<p>editor</p>', success: true },
        { documentPath: 'about.qmd' },
      );
      const last = _getInstalledApiForTesting()!.lastRenderResponse();
      expect(last?.documentPath).toBe('about.qmd');
      expect((last?.result as { html: string }).html).toBe('<p>editor</p>');
    });

    it('lastRenderResponse() resets to null on uninstall', async () => {
      installDebugApi(makeContext());
      await _getInstalledApiForTesting()!.rerender();
      uninstallDebugApi();
      installDebugApi(makeContext());
      expect(_getInstalledApiForTesting()!.lastRenderResponse()).toBeNull();
    });
  });

  describe('active-file routing', () => {
    it('getActiveFile reads from the context', () => {
      installDebugApi(makeContext({ getActiveFile: () => 'about.qmd' }));
      expect(_getInstalledApiForTesting()!.getActiveFile()).toBe('about.qmd');
    });

    it('setActiveFile invokes the context setter for known files', () => {
      const setActive = vi.fn();
      installDebugApi(makeContext({ setActiveFile: setActive }));
      _getInstalledApiForTesting()!.setActiveFile('about.qmd');
      expect(setActive).toHaveBeenCalledWith('about.qmd');
    });

    it('setActiveFile rejects unknown files', () => {
      installDebugApi(makeContext());
      expect(() =>
        _getInstalledApiForTesting()!.setActiveFile('does-not-exist.qmd'),
      ).toThrow(/not in the project/);
    });
  });

  describe('vfs accessors', () => {
    it('vfsList returns all paths when no prefix supplied', () => {
      installDebugApi(makeContext());
      expect(_getInstalledApiForTesting()!.vfsList()).toEqual([
        '/project/index.qmd',
        '/.quarto/project-artifacts/styles.css',
      ]);
    });

    it('vfsList filters by prefix', () => {
      installDebugApi(makeContext());
      expect(_getInstalledApiForTesting()!.vfsList('/.quarto/')).toEqual([
        '/.quarto/project-artifacts/styles.css',
      ]);
    });

    it('vfsRead decodes base64 into Uint8Array', () => {
      installDebugApi(makeContext());
      const bytes = _getInstalledApiForTesting()!.vfsRead('/project/index.qmd');
      expect(bytes).toBeInstanceOf(Uint8Array);
      // 'aGk=' base64 = 'hi'
      expect(Array.from(bytes as Uint8Array)).toEqual([0x68, 0x69]);
    });

    it('vfsRead returns null on miss', () => {
      wasmRendererMocks.vfsReadBinaryFile.mockReturnValueOnce({ success: false });
      installDebugApi(makeContext());
      expect(_getInstalledApiForTesting()!.vfsRead('/missing')).toBeNull();
    });
  });
});
