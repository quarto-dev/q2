/**
 * Automerge Sync Service
 *
 * Thin wrapper around @quarto/quarto-sync-client that provides
 * VFS callbacks to keep the WASM virtual filesystem in sync.
 */

import {
  createSyncClient,
  exportProjectAsZip as exportZip,
  type SyncClient,
  type SyncClientCallbacks,
  type Patch,
  type FileEntry,
  type CreateBinaryFileResult,
  type CreateProjectOptions,
  type CreateProjectResult,
  type FilePayload,
} from '@quarto/quarto-sync-client';

import { vfsAddFile, vfsAddBinaryFile, vfsRemoveFile, vfsClear, initWasm } from './wasmRenderer';

// Re-export types for use in other components
export type { Patch, FileEntry, CreateBinaryFileResult, CreateProjectOptions, CreateProjectResult };

// Event handlers for state changes
type FilesChangeHandler = (files: FileEntry[]) => void;
type FileContentHandler = (path: string, content: string, patches: Patch[]) => void;
type BinaryContentHandler = (path: string, content: Uint8Array, mimeType: string) => void;
type ConnectionHandler = (connected: boolean) => void;
type ErrorHandler = (error: Error) => void;

let onFilesChange: FilesChangeHandler | null = null;
let onFileContent: FileContentHandler | null = null;
let onBinaryContent: BinaryContentHandler | null = null;
let onConnectionChange: ConnectionHandler | null = null;
let onError: ErrorHandler | null = null;

// The sync client instance
let client: SyncClient | null = null;

/**
 * Set event handlers for sync events.
 */
export function setSyncHandlers(handlers: {
  onFilesChange?: FilesChangeHandler;
  onFileContent?: FileContentHandler;
  onBinaryContent?: BinaryContentHandler;
  onConnectionChange?: ConnectionHandler;
  onError?: ErrorHandler;
}) {
  if (handlers.onFilesChange) onFilesChange = handlers.onFilesChange;
  if (handlers.onFileContent) onFileContent = handlers.onFileContent;
  if (handlers.onBinaryContent) onBinaryContent = handlers.onBinaryContent;
  if (handlers.onConnectionChange) onConnectionChange = handlers.onConnectionChange;
  if (handlers.onError) onError = handlers.onError;
}

/**
 * Create the sync client with VFS callbacks.
 */
function ensureClient(): SyncClient {
  if (!client) {
    const callbacks: SyncClientCallbacks = {
      onFileAdded: (path: string, file: FilePayload) => {
        if (file.type === 'text') {
          vfsAddFile(path, file.text);
          onFileContent?.(path, file.text, []);
        } else {
          vfsAddBinaryFile(path, file.data);
          onBinaryContent?.(path, file.data, file.mimeType);
        }
      },
      onFileChanged: (path: string, text: string, patches: Patch[]) => {
        vfsAddFile(path, text);
        onFileContent?.(path, text, patches);
      },
      onBinaryChanged: (path: string, data: Uint8Array, mimeType: string) => {
        vfsAddBinaryFile(path, data);
        onBinaryContent?.(path, data, mimeType);
      },
      onFileRemoved: (path: string) => {
        vfsRemoveFile(path);
      },
      onFilesChange: (files: FileEntry[]) => {
        onFilesChange?.(files);
      },
      onConnectionChange: (connected: boolean) => {
        onConnectionChange?.(connected);
      },
      onError: (error: Error) => {
        onError?.(error);
      },
    };
    client = createSyncClient(callbacks);
  }
  return client;
}

/**
 * Connect to a sync server and load a project.
 */
export async function connect(syncServerUrl: string, indexDocId: string): Promise<FileEntry[]> {
  await initWasm();
  vfsClear();
  return ensureClient().connect(syncServerUrl, indexDocId);
}

/**
 * Disconnect from the sync server.
 */
export async function disconnect(): Promise<void> {
  vfsClear();
  if (client) {
    await client.disconnect();
  }
}

/**
 * Check if a file is binary.
 */
export function isFileBinary(path: string): boolean {
  return ensureClient().isFileBinary(path);
}

/**
 * Get the current text content of a file.
 */
export function getFileContent(path: string): string | null {
  return ensureClient().getFileContent(path);
}

/**
 * Get the current binary content of a file.
 */
export function getBinaryFileContent(path: string): { content: Uint8Array; mimeType: string } | null {
  return ensureClient().getBinaryFileContent(path);
}

/**
 * Update the content of a file using incremental text updates.
 */
export function updateFileContent(path: string, content: string): void {
  ensureClient().updateFileContent(path, content);
  // VFS is updated via callback
}

/**
 * Create a new text file in the project.
 */
export async function createFile(path: string, content: string = ''): Promise<void> {
  await ensureClient().createFile(path, content);
  // VFS is updated via callback
}

/**
 * Create a new binary file in the project.
 */
export async function createBinaryFile(
  path: string,
  content: Uint8Array,
  mimeType: string
): Promise<CreateBinaryFileResult> {
  return ensureClient().createBinaryFile(path, content, mimeType);
  // VFS is updated via callback
}

/**
 * Delete a file from the project.
 */
export function deleteFile(path: string): void {
  ensureClient().deleteFile(path);
  // VFS is updated via callback
}

/**
 * Rename a file in the project.
 */
export function renameFile(oldPath: string, newPath: string): void {
  ensureClient().renameFile(oldPath, newPath);
  // VFS is updated via callback
}

/**
 * Check if connected.
 */
export function isConnected(): boolean {
  return client?.isConnected() ?? false;
}

/**
 * Create a new project with the given files.
 */
export async function createNewProject(options: CreateProjectOptions): Promise<CreateProjectResult> {
  await initWasm();
  vfsClear();
  return ensureClient().createNewProject(options);
}

/**
 * Get the DocHandle for a file by path.
 * Used by the presence service for ephemeral messaging.
 */
export function getFileHandle(path: string) {
  return ensureClient().getFileHandle(path);
}

/**
 * Get all current file paths that have handles.
 */
export function getFilePaths(): string[] {
  return ensureClient().getFilePaths();
}

/**
 * Export all project files as a ZIP archive.
 * Returns a Uint8Array containing the ZIP file bytes.
 */
export function exportProjectAsZip(): Uint8Array {
  return exportZip(ensureClient());
}

// ============================================================================
// Testing Utilities
// ============================================================================

/**
 * Reset the sync service state for testing.
 *
 * This function resets all module-level state to initial values,
 * ensuring test isolation. Call this in beforeEach() to prevent
 * state leakage between tests.
 *
 * @internal For testing only - not part of the public API
 */
export function _resetForTesting(): void {
  // Clear the client instance (will be recreated on next ensureClient() call)
  client = null;

  // Clear all event handlers
  onFilesChange = null;
  onFileContent = null;
  onBinaryContent = null;
  onConnectionChange = null;
  onError = null;
}

/**
 * Inject a mock sync client for testing.
 *
 * This allows tests to provide a mock implementation of the SyncClient
 * instead of using the real createSyncClient function.
 *
 * @internal For testing only - not part of the public API
 */
export function _setClientForTesting(mockClient: SyncClient | null): void {
  client = mockClient;
}
