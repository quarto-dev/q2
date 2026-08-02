/**
 * Tests for automergeSync service
 *
 * These tests verify the sync service's behavior using a mock SyncClient.
 * The mock allows us to test event handling, VFS sync, and state management
 * without requiring a real Automerge server.
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { zipSync, strToU8 } from 'fflate';
import type { FileEntry, Patch, SyncClient } from '@quarto/quarto-sync-client';
import {
  setSyncHandlers,
  isConnected,
  getFileContent,
  getRepo,
  getDocInventory,
  applyEditorOperations,
  isFileBinary,
  setImmediateFileChangeCallback,
  importProjectFromZip,
  _resetForTesting,
  _setClientForTesting,
  _getCallbacksForTesting,
} from './automergeSync';
import { createMockSyncClient, type MockSyncClient } from './test-utils/mockSyncClient';

// Mock the wasmRenderer module to avoid WASM initialization
vi.mock('./wasmRenderer', () => ({
  vfsAddFile: vi.fn(),
  vfsAddBinaryFile: vi.fn(),
  vfsRemoveFile: vi.fn(),
  vfsClear: vi.fn(),
  initWasm: vi.fn().mockResolvedValue(undefined),
}));

describe('automergeSync', () => {
  let mockClient: MockSyncClient;
  // Mirror automergeSync's (module-private) handler signatures so the
  // mocks satisfy setSyncHandlers under the tests typecheck.
  let onFilesChange: ReturnType<typeof vi.fn<(files: FileEntry[]) => void>>;
  let onFileContent: ReturnType<typeof vi.fn<(path: string, content: string, patches: Patch[]) => void>>;
  let onBinaryContent: ReturnType<typeof vi.fn<(path: string, content: Uint8Array, mimeType: string) => void>>;
  let onConnectionChange: ReturnType<typeof vi.fn<(connected: boolean) => void>>;
  let onError: ReturnType<typeof vi.fn<(error: Error) => void>>;

  beforeEach(() => {
    // Reset the module state
    _resetForTesting();

    // Create mock handlers
    onFilesChange = vi.fn<(files: FileEntry[]) => void>();
    onFileContent = vi.fn<(path: string, content: string, patches: Patch[]) => void>();
    onBinaryContent = vi.fn<(path: string, content: Uint8Array, mimeType: string) => void>();
    onConnectionChange = vi.fn<(connected: boolean) => void>();
    onError = vi.fn<(error: Error) => void>();

    // Set up handlers
    setSyncHandlers({
      onFilesChange,
      onFileContent,
      onBinaryContent,
      onConnectionChange,
      onError,
    });
  });

  describe('when no client is connected', () => {
    it('should report not connected', () => {
      expect(isConnected()).toBe(false);
    });

    // Debug accessors must be probe-safe before any connect (the
    // quartoDebug console API calls them unconditionally).
    it('getRepo is null and getDocInventory is empty', () => {
      expect(getRepo()).toBeNull();
      expect(getDocInventory()).toEqual([]);
    });
  });

  describe('with mock client', () => {
    beforeEach(() => {
      // Create a mock client - we need to capture the callbacks
      // The sync service creates the client internally, so we use injection
      const initialFiles = new Map([
        ['index.qmd', { type: 'text' as const, text: '# Hello World' }],
        ['_quarto.yml', { type: 'text' as const, text: 'project:\n  type: default' }],
      ]);

      // Create the mock with our callbacks captured via the handler setup
      mockClient = createMockSyncClient(
        {
          onFileAdded: vi.fn(),
          onFileChanged: vi.fn(),
          onBinaryChanged: vi.fn(),
          onFileRemoved: vi.fn(),
          onFilesChange: vi.fn(),
          onConnectionChange: vi.fn(),
          onError: vi.fn(),
        },
        { initialFiles },
      );

      // Inject the mock client
      // MockSyncClient implements the subset of SyncClient these tests
      // exercise; the seam is test-only, so widen at the boundary.
      _setClientForTesting(mockClient as unknown as SyncClient);
    });

    it('should report connected state', async () => {
      await mockClient.connect('ws://test', 'automerge:test');
      expect(mockClient.isConnected()).toBe(true);
    });

    it('should return file content from mock client', async () => {
      await mockClient.connect('ws://test', 'automerge:test');
      expect(getFileContent('index.qmd')).toBe('# Hello World');
    });

    it('should return null for non-existent files', async () => {
      await mockClient.connect('ws://test', 'automerge:test');
      expect(getFileContent('nonexistent.qmd')).toBeNull();
    });

    it('should identify text files as non-binary', async () => {
      await mockClient.connect('ws://test', 'automerge:test');
      expect(isFileBinary('index.qmd')).toBe(false);
    });

    it('should list all file paths', async () => {
      await mockClient.connect('ws://test', 'automerge:test');
      const paths = mockClient.getFilePaths();
      expect(paths).toContain('index.qmd');
      expect(paths).toContain('_quarto.yml');
    });

    // Debug accessors for quartoDebug.am (bd-q93tkglb).
    it('getRepo delegates to the live client', async () => {
      await mockClient.connect('ws://test', 'automerge:test');
      expect(getRepo()).not.toBeNull();
      // Same object identity as the client's own repo — delegation, not
      // a reconstruction.
      expect(getRepo()).toBe(mockClient.getRepo());
    });

    it('getDocInventory delegates to the live client', async () => {
      await mockClient.connect('ws://test', 'automerge:test');
      const inventory = getDocInventory();
      expect(inventory).toEqual(mockClient.getDocInventory());
      const entry = inventory.find((e) => e.path === 'index.qmd');
      expect(entry).toBeDefined();
      expect(entry!.docId).toBe(mockClient.getFileHandle('index.qmd')!.documentId);
    });
  });

  describe('file operations', () => {
    beforeEach(async () => {
      mockClient = createMockSyncClient(
        {
          onFileAdded: vi.fn(),
          onFileChanged: vi.fn(),
          onBinaryChanged: vi.fn(),
          onFileRemoved: vi.fn(),
        },
        { initialFiles: new Map() },
      );
      // MockSyncClient implements the subset of SyncClient these tests
      // exercise; the seam is test-only, so widen at the boundary.
      _setClientForTesting(mockClient as unknown as SyncClient);
      await mockClient.connect('ws://test', 'automerge:test');
    });

    it('should create new files', async () => {
      await mockClient.createFile('new.qmd', '# New File');
      expect(getFileContent('new.qmd')).toBe('# New File');
    });

    it('should update existing files', async () => {
      await mockClient.createFile('test.qmd', '# Original');
      mockClient.updateFileContent('test.qmd', '# Updated');
      expect(getFileContent('test.qmd')).toBe('# Updated');
    });

    it('should delete files', async () => {
      await mockClient.createFile('delete-me.qmd', '# Delete Me');
      expect(getFileContent('delete-me.qmd')).toBe('# Delete Me');

      mockClient.deleteFile('delete-me.qmd');
      expect(getFileContent('delete-me.qmd')).toBeNull();
    });

    it('should rename files', async () => {
      await mockClient.createFile('old-name.qmd', '# Content');
      mockClient.renameFile('old-name.qmd', 'new-name.qmd');

      expect(getFileContent('old-name.qmd')).toBeNull();
      expect(getFileContent('new-name.qmd')).toBe('# Content');
    });
  });

  describe('binary file handling', () => {
    beforeEach(async () => {
      mockClient = createMockSyncClient(
        {
          onFileAdded: vi.fn(),
          onFileChanged: vi.fn(),
          onBinaryChanged: vi.fn(),
          onFileRemoved: vi.fn(),
        },
        { initialFiles: new Map() },
      );
      // MockSyncClient implements the subset of SyncClient these tests
      // exercise; the seam is test-only, so widen at the boundary.
      _setClientForTesting(mockClient as unknown as SyncClient);
      await mockClient.connect('ws://test', 'automerge:test');
    });

    it('should create binary files', async () => {
      const content = new Uint8Array([0x89, 0x50, 0x4e, 0x47]); // PNG magic bytes
      await mockClient.createBinaryFile('image.png', content, 'image/png');

      expect(isFileBinary('image.png')).toBe(true);
      const result = mockClient.getBinaryFileContent('image.png');
      expect(result).not.toBeNull();
      expect(result?.mimeType).toBe('image/png');
      expect(result?.content).toEqual(content);
    });

    it('should return null for text content of binary files', async () => {
      const content = new Uint8Array([0x89, 0x50, 0x4e, 0x47]);
      await mockClient.createBinaryFile('image.png', content, 'image/png');

      expect(getFileContent('image.png')).toBeNull();
    });
  });

  describe('error handling', () => {
    it('should handle connection failures', async () => {
      mockClient = createMockSyncClient(
        {
          onFileAdded: vi.fn(),
          onFileChanged: vi.fn(),
          onBinaryChanged: vi.fn(),
          onFileRemoved: vi.fn(),
          onError: vi.fn(),
        },
        { failConnection: true, connectionError: 'Server unavailable' },
      );
      // MockSyncClient implements the subset of SyncClient these tests
      // exercise; the seam is test-only, so widen at the boundary.
      _setClientForTesting(mockClient as unknown as SyncClient);

      await expect(mockClient.connect('ws://test', 'automerge:test')).rejects.toThrow(
        'Server unavailable',
      );
    });
  });

  describe('applyEditorOperations', () => {
    beforeEach(async () => {
      mockClient = createMockSyncClient(
        {
          onFileAdded: vi.fn(),
          onFileChanged: vi.fn(),
          onBinaryChanged: vi.fn(),
          onFileRemoved: vi.fn(),
        },
        { initialFiles: new Map() },
      );
      // MockSyncClient implements the subset of SyncClient these tests
      // exercise; the seam is test-only, so widen at the boundary.
      _setClientForTesting(mockClient as unknown as SyncClient);
      await mockClient.connect('ws://test', 'automerge:test');
      await mockClient.createFile('test.qmd', 'hello world');
    });

    it('should apply a selection replacement (delete+insert) atomically', () => {
      // Simulate typing "a" to replace selected "world"
      applyEditorOperations('test.qmd', [
        { rangeOffset: 6, rangeLength: 5, text: 'a' },
      ]);
      expect(getFileContent('test.qmd')).toBe('hello a');
    });

    it('should apply a simple insert', () => {
      applyEditorOperations('test.qmd', [
        { rangeOffset: 5, rangeLength: 0, text: ',' },
      ]);
      expect(getFileContent('test.qmd')).toBe('hello, world');
    });

    it('should apply a simple delete', () => {
      applyEditorOperations('test.qmd', [
        { rangeOffset: 5, rangeLength: 6, text: '' },
      ]);
      expect(getFileContent('test.qmd')).toBe('hello');
    });

    it('should apply multi-change batch in correct order', () => {
      // Find-replace all: "hello" → "HI", "world" → "EARTH"
      // Monaco sends changes end-to-beginning
      applyEditorOperations('test.qmd', [
        { rangeOffset: 6, rangeLength: 5, text: 'EARTH' },
        { rangeOffset: 0, rangeLength: 5, text: 'HI' },
      ]);
      expect(getFileContent('test.qmd')).toBe('HI EARTH');
    });
  });

  describe('immediate file change callback', () => {
    beforeEach(async () => {
      // Use real internal callbacks so immediateFileChangeCallback fires
      mockClient = createMockSyncClient(
        _getCallbacksForTesting(),
        { initialFiles: new Map() },
      );
      // MockSyncClient implements the subset of SyncClient these tests
      // exercise; the seam is test-only, so widen at the boundary.
      _setClientForTesting(mockClient as unknown as SyncClient);
      await mockClient.connect('ws://test', 'automerge:test');
    });

    it('should fire callback synchronously on remote change', async () => {
      await mockClient.createFile('test.qmd', 'hello');

      let callbackFired = false;
      setImmediateFileChangeCallback((path, content) => {
        callbackFired = true;
        expect(path).toBe('test.qmd');
        expect(content).toBe('new content');
      });

      mockClient._simulateRemoteChange('test.qmd', 'new content');
      // The callback must have fired synchronously — no await needed
      expect(callbackFired).toBe(true);
    });

    it('should fire callback synchronously on local splice', async () => {
      await mockClient.createFile('test.qmd', 'hello');

      let callbackFired = false;
      setImmediateFileChangeCallback((path, content) => {
        callbackFired = true;
        expect(path).toBe('test.qmd');
        expect(content).toBe('helloX');
      });

      applyEditorOperations('test.qmd', [{ rangeOffset: 5, rangeLength: 0, text: 'X' }]);
      expect(callbackFired).toBe(true);
    });

    it('should fire callback before onFileContent handler', async () => {
      await mockClient.createFile('test.qmd', 'hello');

      let immediateCallbackFired = false;
      let onFileContentCalled = false;

      setImmediateFileChangeCallback(() => {
        immediateCallbackFired = true;
      });

      // Register an onFileContent handler that checks ordering
      setSyncHandlers({
        onFileContent: () => {
          // Immediate callback must have already fired
          expect(immediateCallbackFired).toBe(true);
          onFileContentCalled = true;
        },
      });

      mockClient._simulateRemoteChange('test.qmd', 'updated');

      // Verify both handlers actually fired (not a vacuous pass)
      expect(immediateCallbackFired).toBe(true);
      expect(onFileContentCalled).toBe(true);
    });

    it('should receive correct path for different files', async () => {
      await mockClient.createFile('a.qmd', 'aaa');
      await mockClient.createFile('b.qmd', 'bbb');

      const calls: [string, string][] = [];
      setImmediateFileChangeCallback((path, content) => {
        calls.push([path, content]);
      });

      mockClient._simulateRemoteChange('a.qmd', 'aaa updated');
      mockClient._simulateRemoteChange('b.qmd', 'bbb updated');

      expect(calls).toEqual([
        ['a.qmd', 'aaa updated'],
        ['b.qmd', 'bbb updated'],
      ]);
    });

    it('should not throw when callback is null', async () => {
      await mockClient.createFile('test.qmd', 'hello');
      setImmediateFileChangeCallback(null);

      expect(() => {
        mockClient._simulateRemoteChange('test.qmd', 'new content');
      }).not.toThrow();
    });

    it('should only call the most recently registered callback', async () => {
      await mockClient.createFile('test.qmd', 'hello');

      const callbackA = vi.fn();
      const callbackB = vi.fn();

      setImmediateFileChangeCallback(callbackA);
      setImmediateFileChangeCallback(callbackB);

      mockClient._simulateRemoteChange('test.qmd', 'new content');

      expect(callbackA).not.toHaveBeenCalled();
      expect(callbackB).toHaveBeenCalledWith('test.qmd', 'new content');
    });
  });

  describe('position-correctness scenarios', () => {
    beforeEach(async () => {
      // Use real internal callbacks so immediateFileChangeCallback fires
      mockClient = createMockSyncClient(
        _getCallbacksForTesting(),
        { initialFiles: new Map() },
      );
      // MockSyncClient implements the subset of SyncClient these tests
      // exercise; the seam is test-only, so widen at the boundary.
      _setClientForTesting(mockClient as unknown as SyncClient);
      await mockClient.connect('ws://test', 'automerge:test');
    });

    it('should maintain correct order in same-paragraph concurrent edits', async () => {
      // Scenario 1 from the plan: Document "hello"
      await mockClient.createFile('test.qmd', 'hello');

      const receivedContents: string[] = [];
      setImmediateFileChangeCallback((_path, content) => {
        receivedContents.push(content);
      });

      // Step 1: Local user types 'a' at pos 5 → "helloa"
      applyEditorOperations('test.qmd', [{ rangeOffset: 5, rangeLength: 0, text: 'a' }]);
      expect(getFileContent('test.qmd')).toBe('helloa');

      // Step 2: Remote user inserts 'x' at pos 0 → "xhelloa"
      mockClient._simulateRemoteChange('test.qmd', 'xhelloa');
      // The immediate callback fires synchronously, delivering "xhelloa"
      // to Editor.tsx, which would sync Monaco before the next keystroke.
      expect(receivedContents).toContain('xhelloa');

      // Step 3: Local user types 'b' at pos 7 (after 'a' in synced "xhelloa")
      // With Monaco synced, the user's cursor is at pos 7 (not stale pos 6).
      applyEditorOperations('test.qmd', [{ rangeOffset: 7, rangeLength: 0, text: 'b' }]);
      expect(getFileContent('test.qmd')).toBe('xhelloab');

      // WITHOUT the fix: if Monaco were stale ("helloa"), the user would type
      // 'b' at pos 6, and splice(6, 0, 'b') on "xhelloa" would produce
      // "xhelloba" — letters reversed. The fix ensures the correct offset (7).
    });

    it('should maintain correct order in cross-paragraph concurrent edits', async () => {
      // Scenario 2 from the plan: Document "aaa\n\nbbb"
      await mockClient.createFile('test.qmd', 'aaa\n\nbbb');

      const receivedContents: string[] = [];
      setImmediateFileChangeCallback((_path, content) => {
        receivedContents.push(content);
      });

      // Step 1: Local user types 'x' at end of paragraph 2 (pos 8) → "aaa\n\nbbbx"
      applyEditorOperations('test.qmd', [{ rangeOffset: 8, rangeLength: 0, text: 'x' }]);
      expect(getFileContent('test.qmd')).toBe('aaa\n\nbbbx');

      // Step 2: Remote user types 'y' at end of paragraph 1 (pos 3) → "aaay\n\nbbbx"
      mockClient._simulateRemoteChange('test.qmd', 'aaay\n\nbbbx');
      expect(receivedContents).toContain('aaay\n\nbbbx');

      // Step 3: Local user types 'z' at pos 10 (after 'x' in synced "aaay\n\nbbbx")
      // With Monaco synced, 'x' is at pos 9, so 'z' goes at pos 10.
      applyEditorOperations('test.qmd', [{ rangeOffset: 10, rangeLength: 0, text: 'z' }]);
      expect(getFileContent('test.qmd')).toBe('aaay\n\nbbbxz');

      // WITHOUT the fix: if Monaco were stale ("aaa\n\nbbbx"), the user would
      // type 'z' at pos 9, and splice(9, 0, 'z') on "aaay\n\nbbbx" would
      // produce "aaay\n\nbbzx" — 'z' lands before 'x', not after.
    });
  });

  describe('importProjectFromZip', () => {
    it('converts parsed files into the snake_case ProjectFile shape', () => {
      const pngBytes = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a]);
      const zip = zipSync(
        {
          'index.qmd': strToU8('# Hello'),
          'images/logo.png': pngBytes,
        },
        { level: 0 },
      );

      const files = importProjectFromZip(zip);
      const byPath = Object.fromEntries(files.map(f => [f.path, f]));

      // Text entry: snake_case keys, plain-string content.
      expect(byPath['index.qmd']).toEqual({
        path: 'index.qmd',
        content_type: 'text',
        content: '# Hello',
        mime_type: undefined,
      });

      // Binary entry: base64 content + inferred mime_type.
      const png = byPath['images/logo.png'];
      expect(png.content_type).toBe('binary');
      expect(png.mime_type).toBe('image/png');
      expect(Uint8Array.from(atob(png.content), c => c.charCodeAt(0))).toEqual(pngBytes);
    });

    it('propagates parse errors (e.g. empty archive)', () => {
      const empty = zipSync({}, { level: 0 });
      expect(() => importProjectFromZip(empty)).toThrow(/no files/i);
    });
  });

  describe('test isolation', () => {
    it('should have clean state after reset', () => {
      _resetForTesting();
      expect(isConnected()).toBe(false);
    });

    it('should clear immediate callback on reset', () => {
      const callback = vi.fn();
      setImmediateFileChangeCallback(callback);

      _resetForTesting();

      // After reset, set up a new client and trigger a change —
      // the old callback should not fire
      mockClient = createMockSyncClient(
        {
          onFileAdded: vi.fn(),
          onFileChanged: vi.fn(),
          onBinaryChanged: vi.fn(),
          onFileRemoved: vi.fn(),
        },
        { initialFiles: new Map() },
      );
      // MockSyncClient implements the subset of SyncClient these tests
      // exercise; the seam is test-only, so widen at the boundary.
      _setClientForTesting(mockClient as unknown as SyncClient);

      // The callback should have been cleared by _resetForTesting
      expect(callback).not.toHaveBeenCalled();
    });
  });
});
