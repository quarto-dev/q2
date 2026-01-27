/**
 * Tests for automergeSync service
 *
 * These tests verify the sync service's behavior using a mock SyncClient.
 * The mock allows us to test event handling, VFS sync, and state management
 * without requiring a real Automerge server.
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';
import type { FileEntry, Patch } from '@quarto/quarto-sync-client';
import {
  setSyncHandlers,
  isConnected,
  getFileContent,
  isFileBinary,
  _resetForTesting,
  _setClientForTesting,
} from './automergeSync';
import { createMockSyncClient, type MockSyncClient } from '../test-utils/mockSyncClient';

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
  let onFilesChange: ReturnType<typeof vi.fn>;
  let onFileContent: ReturnType<typeof vi.fn>;
  let onBinaryContent: ReturnType<typeof vi.fn>;
  let onConnectionChange: ReturnType<typeof vi.fn>;
  let onError: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    // Reset the module state
    _resetForTesting();

    // Create mock handlers
    onFilesChange = vi.fn();
    onFileContent = vi.fn();
    onBinaryContent = vi.fn();
    onConnectionChange = vi.fn();
    onError = vi.fn();

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
      _setClientForTesting(mockClient);
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
      _setClientForTesting(mockClient);
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
      _setClientForTesting(mockClient);
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
      _setClientForTesting(mockClient);

      await expect(mockClient.connect('ws://test', 'automerge:test')).rejects.toThrow(
        'Server unavailable',
      );
    });
  });

  describe('test isolation', () => {
    it('should have clean state after reset', () => {
      _resetForTesting();
      expect(isConnected()).toBe(false);
    });
  });
});
