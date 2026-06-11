/**
 * Automerge Sync Service
 *
 * Thin wrapper around @quarto/quarto-sync-client that provides
 * VFS callbacks to keep the WASM virtual filesystem in sync.
 */

import {
  createSyncClient,
  exportProjectAsZip as exportZip,
  parseProjectZip,
  type SyncClient,
  type SyncClientCallbacks,
  type Patch,
  type EditorContentChange,
  type FileEntry,
  type ActorIdentity,
  type CaptureRef,
  type CreateBinaryFileResult,
  type CreateProjectOptions,
  type CreateProjectResult,
  type FilePayload,
} from '@quarto/quarto-sync-client';

import { vfsAddFile, vfsAddBinaryFile, vfsRemoveFile, vfsClearAsync, initWasm, type ProjectFile } from './wasmRenderer';

// Re-export types for use in other components
export type { Patch, EditorContentChange, FileEntry, ActorIdentity, CaptureRef, CreateBinaryFileResult, CreateProjectOptions, CreateProjectResult };

// Event handlers for state changes
type FilesChangeHandler = (files: FileEntry[]) => void;
type IdentitiesChangeHandler = (identities: Record<string, ActorIdentity>) => void;
type CapturesChangeHandler = (captures: Record<string, CaptureRef>) => void;
type FileContentHandler = (path: string, content: string, patches: Patch[]) => void;
type BinaryContentHandler = (path: string, content: Uint8Array, mimeType: string) => void;
type ConnectionHandler = (connected: boolean) => void;
type ErrorHandler = (error: Error) => void;

let onFilesChange: FilesChangeHandler | null = null;
let onIdentitiesChange: IdentitiesChangeHandler | null = null;
let onCapturesChange: CapturesChangeHandler | null = null;
let onFileContent: FileContentHandler | null = null;
let onBinaryContent: BinaryContentHandler | null = null;
let onConnectionChange: ConnectionHandler | null = null;
let onError: ErrorHandler | null = null;

// Synchronous callback that fires from within onFileChanged, BEFORE the React
// state update path. Used by useAutomergeSync to apply remote diffs to Monaco
// immediately (real-time remote edit path), preventing position mismatch when
// keystrokes interleave with async React state updates.
type ImmediateFileChangeCallback = (path: string, content: string) => void;
let immediateFileChangeCallback: ImmediateFileChangeCallback | null = null;

export function setImmediateFileChangeCallback(cb: ImmediateFileChangeCallback | null): void {
  immediateFileChangeCallback = cb;
}

// The sync client instance
let client: SyncClient | null = null;

/**
 * Set event handlers for sync events.
 */
export function setSyncHandlers(handlers: {
  onFilesChange?: FilesChangeHandler;
  onIdentitiesChange?: IdentitiesChangeHandler;
  onCapturesChange?: CapturesChangeHandler;
  onFileContent?: FileContentHandler;
  onBinaryContent?: BinaryContentHandler;
  onConnectionChange?: ConnectionHandler;
  onError?: ErrorHandler;
}) {
  if (handlers.onFilesChange) onFilesChange = handlers.onFilesChange;
  if (handlers.onIdentitiesChange) onIdentitiesChange = handlers.onIdentitiesChange;
  if (handlers.onCapturesChange) onCapturesChange = handlers.onCapturesChange;
  if (handlers.onFileContent) onFileContent = handlers.onFileContent;
  if (handlers.onBinaryContent) onBinaryContent = handlers.onBinaryContent;
  if (handlers.onConnectionChange) onConnectionChange = handlers.onConnectionChange;
  if (handlers.onError) onError = handlers.onError;
}

/**
 * Create the internal callback chain that bridges SyncClient events to
 * module-level handlers (VFS updates, immediate callback, React state).
 */
function createInternalCallbacks(): SyncClientCallbacks {
  return {
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
      immediateFileChangeCallback?.(path, text);
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
    onIdentitiesChange: (identities: Record<string, ActorIdentity>) => {
      onIdentitiesChange?.(identities);
    },
    onCapturesChange: (captures: Record<string, CaptureRef>) => {
      onCapturesChange?.(captures);
    },
    onConnectionChange: (connected: boolean) => {
      onConnectionChange?.(connected);
    },
    onError: (error: Error) => {
      onError?.(error);
    },
  };
}

/**
 * Create the sync client with VFS callbacks.
 */
function ensureClient(): SyncClient {
  if (!client) {
    client = createSyncClient(createInternalCallbacks());
  }
  return client;
}

/**
 * Connect to a sync server and load a project.
 *
 * Auth is handled via HttpOnly cookies, sent automatically by the
 * browser on same-origin WebSocket upgrades.
 *
 * `peerTimeoutMs` controls how long to wait for the samod handshake
 * before falling through to offline-from-IndexedDB mode. The
 * underlying default (1 ms) is appropriate for hub-client where
 * IndexedDB usually has cached docs from a prior session. The
 * q2-preview SPA hits a fresh ephemeral hub with no IndexedDB
 * cache, so it passes a longer timeout to avoid an `findDoc`
 * "unavailable" race on cold start.
 */
export async function connect(syncServerUrl: string, indexDocId: string, actorId?: string, screenName?: string, color?: string, peerTimeoutMs?: number): Promise<FileEntry[]> {
  await initWasm();
  const t0 = Date.now();
  await vfsClearAsync();
  const waited = Date.now() - t0;
  if (waited > 10) console.warn(`[exp/connect] vfsClearAsync waited ${waited}ms`);

  return ensureClient().connect(syncServerUrl, indexDocId, actorId, screenName, color, peerTimeoutMs);
}

/**
 * Disconnect from the sync server.
 */
export async function disconnect(): Promise<void> {
  const t0 = Date.now();
  await vfsClearAsync();
  const waited = Date.now() - t0;
  if (waited > 10) console.warn(`[exp/disconnect] vfsClearAsync waited ${waited}ms`);
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
 * Fetch a binary document by samod document ID (not by path).
 *
 * Used by Phase C.4 (bd-kw93.3) to resolve capture binary docs
 * referenced from the IndexDocument's V2 capture sidecar.
 * Returns `null` for unknown ids or non-binary documents.
 */
export async function getBinaryDocById(
  docId: string,
): Promise<{ content: Uint8Array; mimeType: string } | null> {
  return ensureClient().getBinaryDocById(docId);
}

/**
 * Update the content of a file using incremental text updates.
 */
export function updateFileContent(path: string, content: string): void {
  ensureClient().updateFileContent(path, content);
  // VFS is updated via callback
}

/**
 * Apply positional editor operations (splice-based) to a text file.
 * Each change maps directly to an Automerge splice, avoiding the
 * full-text diff race in updateText.
 */
export function applyEditorOperations(path: string, changes: EditorContentChange[]): void {
  ensureClient().applyEditorOperations(path, changes);
  // VFS is updated via callback (change handler fires onFileChanged)
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
export async function createNewProject(
  options: CreateProjectOptions,
  actorId?: string,
  screenName?: string,
  color?: string,
  resolveActorId?: (indexDocId: string) => Promise<string | null | undefined>,
): Promise<CreateProjectResult> {
  await initWasm();
  const t0 = Date.now();
  await vfsClearAsync();
  const waited = Date.now() - t0;
  if (waited > 10) console.warn(`[exp/createNewProject] vfsClearAsync waited ${waited}ms`);

  return ensureClient().createNewProject(options, actorId, screenName, color, resolveActorId);
}

/**
 * Get the current actor ID, or null if not set.
 */
export function getActorId(): string | null {
  return client?.getActorId() ?? null;
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

/**
 * Parse a ZIP archive into the scaffold-file shape used by the
 * "create new project" flow.
 *
 * The inverse of {@link exportProjectAsZip}: it produces `ProjectFile[]`
 * (the same snake_case shape `create_project` returns from WASM), so the
 * result flows straight into the existing project-creation callback
 * without any further mapping. Pure — does not need a connected client.
 *
 * @param zipBytes - Raw bytes of an uploaded `.zip`
 * @returns Files ready to hand to the project-creation path
 * @throws If the archive is corrupt, contains an unsafe path, or yields
 *   no usable files.
 */
export function importProjectFromZip(zipBytes: Uint8Array): ProjectFile[] {
  return parseProjectZip(zipBytes).map(f => ({
    path: f.path,
    content_type: f.contentType,
    content: f.content,
    mime_type: f.mimeType,
  }));
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
  onIdentitiesChange = null;
  onFileContent = null;
  onBinaryContent = null;
  onConnectionChange = null;
  onError = null;
  immediateFileChangeCallback = null;
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

/**
 * Get the internal callback chain for use with mock clients in tests.
 * @internal For testing only - not part of the public API
 */
export function _getCallbacksForTesting(): SyncClientCallbacks {
  return createInternalCallbacks();
}
