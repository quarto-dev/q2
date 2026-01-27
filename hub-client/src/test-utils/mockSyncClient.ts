/**
 * Mock Sync Client for Testing
 *
 * Provides a configurable mock implementation of the SyncClient interface
 * for unit and integration tests. Supports:
 * - Programmatic file operations for test setup
 * - Callback invocation for testing event handlers
 * - Connection state simulation
 * - Error injection for error handling tests
 */

import type { Patch } from '@automerge/automerge-repo';
import type {
  SyncClientCallbacks,
  TextFilePayload,
  BinaryFilePayload,
  FilePayload,
  CreateBinaryFileResult,
  CreateProjectOptions,
  CreateProjectResult,
} from '@quarto/quarto-sync-client';
import type { FileEntry } from '@quarto/quarto-automerge-schema';

/**
 * Options for configuring the mock sync client.
 */
export interface MockSyncClientOptions {
  /** Initial files to populate the mock VFS */
  initialFiles?: Map<string, FilePayload>;
  /** Simulated connection delay in ms */
  connectionDelay?: number;
  /** Whether to fail connection attempts */
  failConnection?: boolean;
  /** Custom error message for failed connections */
  connectionError?: string;
}

/**
 * Extended SyncClient interface with test helpers.
 */
export interface MockSyncClient {
  // Standard SyncClient methods
  connect(syncServerUrl: string, indexDocId: string): Promise<FileEntry[]>;
  disconnect(): Promise<void>;
  isConnected(): boolean;
  isFileBinary(path: string): boolean;
  getFileContent(path: string): string | null;
  getBinaryFileContent(path: string): { content: Uint8Array; mimeType: string } | null;
  updateFileContent(path: string, content: string): void;
  createFile(path: string, content?: string): Promise<void>;
  createBinaryFile(path: string, content: Uint8Array, mimeType: string): Promise<CreateBinaryFileResult>;
  deleteFile(path: string): void;
  renameFile(oldPath: string, newPath: string): void;
  getFileHandle(path: string): { documentId: string } | null;
  getFilePaths(): string[];
  createNewProject(options: CreateProjectOptions): Promise<CreateProjectResult>;

  // Test helpers
  _simulateRemoteChange(path: string, content: string, patches?: Patch[]): void;
  _simulateBinaryChange(path: string, content: Uint8Array, mimeType: string): void;
  _simulateFileAdded(path: string, content: FilePayload): void;
  _simulateFileRemoved(path: string): void;
  _simulateConnectionChange(connected: boolean): void;
  _simulateError(error: Error): void;
  _getFiles(): Map<string, FilePayload>;
  _reset(): void;
}

/**
 * Create a mock sync client for testing.
 *
 * @param callbacks - The callbacks to invoke on events
 * @param options - Configuration options
 * @returns A mock sync client with test helpers
 *
 * @example
 * ```typescript
 * const callbacks: SyncClientCallbacks = {
 *   onFileAdded: vi.fn(),
 *   onFileChanged: vi.fn(),
 *   onBinaryChanged: vi.fn(),
 *   onFileRemoved: vi.fn(),
 * };
 *
 * const client = createMockSyncClient(callbacks, {
 *   initialFiles: new Map([
 *     ['index.qmd', { type: 'text', text: '# Hello' }],
 *   ]),
 * });
 *
 * await client.connect('ws://localhost:3030', 'automerge:test');
 * expect(callbacks.onFileAdded).toHaveBeenCalledWith('index.qmd', expect.any(Object));
 * ```
 */
export function createMockSyncClient(
  callbacks: SyncClientCallbacks,
  options: MockSyncClientOptions = {},
): MockSyncClient {
  const files = new Map<string, FilePayload>(options.initialFiles || []);
  let connected = false;
  const fileHandles = new Map<string, { documentId: string }>();
  let docIdCounter = 0;

  const generateDocId = () => `automerge:test-${docIdCounter++}`;

  const client: MockSyncClient = {
    async connect(_syncServerUrl: string, _indexDocId: string): Promise<FileEntry[]> {
      if (options.failConnection) {
        const error = new Error(options.connectionError || 'Connection failed');
        callbacks.onError?.(error);
        throw error;
      }

      if (options.connectionDelay) {
        await new Promise((r) => setTimeout(r, options.connectionDelay));
      }

      connected = true;
      callbacks.onConnectionChange?.(true);

      const entries: FileEntry[] = [];
      for (const [path, content] of files) {
        const docId = generateDocId();
        fileHandles.set(path, { documentId: docId });
        entries.push({ path, docId });
        callbacks.onFileAdded(path, content);
      }

      callbacks.onFilesChange?.(entries);
      return entries;
    },

    async disconnect(): Promise<void> {
      for (const path of files.keys()) {
        callbacks.onFileRemoved(path);
      }
      connected = false;
      callbacks.onConnectionChange?.(false);
    },

    isConnected: () => connected,

    isFileBinary(path: string): boolean {
      const file = files.get(path);
      return file?.type === 'binary';
    },

    getFileContent(path: string): string | null {
      const file = files.get(path);
      if (!file || file.type !== 'text') return null;
      return file.text;
    },

    getBinaryFileContent(path: string): { content: Uint8Array; mimeType: string } | null {
      const file = files.get(path);
      if (!file || file.type !== 'binary') return null;
      return { content: file.data, mimeType: file.mimeType };
    },

    updateFileContent(path: string, content: string): void {
      const existing = files.get(path);
      if (!existing || existing.type !== 'text') {
        throw new Error(`Cannot update non-text file: ${path}`);
      }
      files.set(path, { type: 'text', text: content });
      callbacks.onFileChanged(path, content, []);
    },

    async createFile(path: string, content: string = ''): Promise<void> {
      const payload: TextFilePayload = { type: 'text', text: content };
      files.set(path, payload);
      const docId = generateDocId();
      fileHandles.set(path, { documentId: docId });
      callbacks.onFileAdded(path, payload);
    },

    async createBinaryFile(
      path: string,
      content: Uint8Array,
      mimeType: string,
    ): Promise<CreateBinaryFileResult> {
      const payload: BinaryFilePayload = { type: 'binary', data: content, mimeType };
      files.set(path, payload);
      const docId = generateDocId();
      fileHandles.set(path, { documentId: docId });
      callbacks.onFileAdded(path, payload);
      return { docId, path, deduplicated: false };
    },

    deleteFile(path: string): void {
      files.delete(path);
      fileHandles.delete(path);
      callbacks.onFileRemoved(path);
    },

    renameFile(oldPath: string, newPath: string): void {
      const file = files.get(oldPath);
      if (!file) throw new Error(`File not found: ${oldPath}`);

      const handle = fileHandles.get(oldPath);

      files.delete(oldPath);
      files.set(newPath, file);

      fileHandles.delete(oldPath);
      if (handle) {
        fileHandles.set(newPath, handle);
      }

      callbacks.onFileRemoved(oldPath);
      callbacks.onFileAdded(newPath, file);
    },

    getFileHandle(path: string): { documentId: string } | null {
      return fileHandles.get(path) ?? null;
    },

    getFilePaths(): string[] {
      return Array.from(files.keys());
    },

    async createNewProject(options: CreateProjectOptions): Promise<CreateProjectResult> {
      // Clear existing state
      files.clear();
      fileHandles.clear();

      const createdFiles: FileEntry[] = [];

      for (const file of options.files) {
        const docId = generateDocId();
        fileHandles.set(file.path, { documentId: docId });
        createdFiles.push({ path: file.path, docId });

        if (file.contentType === 'binary') {
          const binaryContent = Uint8Array.from(atob(file.content), (c) => c.charCodeAt(0));
          const mimeType = file.mimeType || 'application/octet-stream';
          const payload: BinaryFilePayload = { type: 'binary', data: binaryContent, mimeType };
          files.set(file.path, payload);
          callbacks.onFileAdded(file.path, payload);
        } else {
          const payload: TextFilePayload = { type: 'text', text: file.content };
          files.set(file.path, payload);
          callbacks.onFileAdded(file.path, payload);
        }
      }

      connected = true;
      callbacks.onConnectionChange?.(true);

      return {
        indexDocId: generateDocId(),
        files: createdFiles,
      };
    },

    // Test helpers
    _simulateRemoteChange(path: string, content: string, patches: Patch[] = []): void {
      files.set(path, { type: 'text', text: content });
      callbacks.onFileChanged(path, content, patches);
    },

    _simulateBinaryChange(path: string, content: Uint8Array, mimeType: string): void {
      files.set(path, { type: 'binary', data: content, mimeType });
      callbacks.onBinaryChanged(path, content, mimeType);
    },

    _simulateFileAdded(path: string, content: FilePayload): void {
      files.set(path, content);
      const docId = generateDocId();
      fileHandles.set(path, { documentId: docId });
      callbacks.onFileAdded(path, content);
    },

    _simulateFileRemoved(path: string): void {
      files.delete(path);
      fileHandles.delete(path);
      callbacks.onFileRemoved(path);
    },

    _simulateConnectionChange(isConnected: boolean): void {
      connected = isConnected;
      callbacks.onConnectionChange?.(isConnected);
    },

    _simulateError(error: Error): void {
      callbacks.onError?.(error);
    },

    _getFiles(): Map<string, FilePayload> {
      return new Map(files);
    },

    _reset(): void {
      files.clear();
      fileHandles.clear();
      connected = false;
      docIdCounter = 0;
    },
  };

  return client;
}
