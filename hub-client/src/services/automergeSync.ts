/**
 * Automerge Sync Service
 *
 * Manages real-time document synchronization using automerge-repo.
 * Handles connection to sync servers and document state management.
 */

import { Repo, DocHandle } from '@automerge/automerge-repo';
import type { DocumentId, Patch } from '@automerge/automerge-repo';
import { updateText } from '@automerge/automerge';

// Re-export Patch type for use in other components
export type { Patch };
import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import type { FileEntry } from '../types/project';
import { vfsAddFile, vfsRemoveFile, vfsClear, initWasm } from './wasmRenderer';

// Document types
interface IndexDocument {
  files: Record<string, string>; // path -> docId mapping
}

interface FileDocument {
  text: string; // automerge Text type serializes to string
}

// Connection state
interface SyncState {
  repo: Repo | null;
  wsAdapter: BrowserWebSocketClientAdapter | null;
  indexHandle: DocHandle<IndexDocument> | null;
  fileHandles: Map<string, DocHandle<FileDocument>>;
  cleanupFns: (() => void)[];
}

const state: SyncState = {
  repo: null,
  wsAdapter: null,
  indexHandle: null,
  fileHandles: new Map(),
  cleanupFns: [],
};

// Event handlers for state changes
type FilesChangeHandler = (files: FileEntry[]) => void;
type FileContentHandler = (path: string, content: string, patches: Patch[]) => void;
type ConnectionHandler = (connected: boolean) => void;
type ErrorHandler = (error: Error) => void;

let onFilesChange: FilesChangeHandler | null = null;
let onFileContent: FileContentHandler | null = null;
let onConnectionChange: ConnectionHandler | null = null;
let onError: ErrorHandler | null = null;

/**
 * Set event handlers for sync events
 */
export function setSyncHandlers(handlers: {
  onFilesChange?: FilesChangeHandler;
  onFileContent?: FileContentHandler;
  onConnectionChange?: ConnectionHandler;
  onError?: ErrorHandler;
}) {
  if (handlers.onFilesChange) onFilesChange = handlers.onFilesChange;
  if (handlers.onFileContent) onFileContent = handlers.onFileContent;
  if (handlers.onConnectionChange) onConnectionChange = handlers.onConnectionChange;
  if (handlers.onError) onError = handlers.onError;
}

/**
 * Connect to a sync server and load a project
 */
export async function connect(syncServerUrl: string, indexDocId: string): Promise<FileEntry[]> {
  // Ensure WASM is initialized for VFS operations
  await initWasm();

  // Disconnect from any existing connection
  await disconnect();

  try {
    // Create WebSocket adapter
    state.wsAdapter = new BrowserWebSocketClientAdapter(syncServerUrl);

    // Create repo with the network adapter
    state.repo = new Repo({
      network: [state.wsAdapter],
    });

    // Load the index document
    const docId = indexDocId as DocumentId;
    console.log('Looking for index document:', docId);
    const indexHandle = await state.repo.find<IndexDocument>(docId);
    state.indexHandle = indexHandle;

    // Wait for the document to be ready
    console.log('Waiting for document to be ready...');
    await indexHandle.whenReady();
    console.log('Document ready');

    // Get initial file list
    const doc = indexHandle.doc();
    console.log('Document content:', doc);
    console.log('Document files field:', doc?.files);
    if (!doc) {
      throw new Error('Failed to load index document');
    }

    const files = getFilesFromIndex(doc);
    console.log('Parsed files:', files);

    // Subscribe to index changes using eventemitter3 pattern
    const indexChangeHandler = () => {
      const changedDoc = indexHandle.doc();
      if (changedDoc) {
        const newFiles = getFilesFromIndex(changedDoc);
        syncVfsWithFiles(newFiles);
        onFilesChange?.(newFiles);
      }
    };
    indexHandle.on('change', indexChangeHandler);
    state.cleanupFns.push(() => indexHandle.off('change', indexChangeHandler));

    // Load and subscribe to all file documents
    await loadFileDocuments(files);

    onConnectionChange?.(true);
    return files;
  } catch (err) {
    const error = err instanceof Error ? err : new Error(String(err));
    onError?.(error);
    throw error;
  }
}

/**
 * Disconnect from the sync server
 */
export async function disconnect(): Promise<void> {
  // Clean up subscriptions
  for (const cleanup of state.cleanupFns) {
    cleanup();
  }
  state.cleanupFns = [];

  // Clear file handles
  state.fileHandles.clear();

  // Clear VFS
  vfsClear();

  // Disconnect WebSocket
  if (state.wsAdapter) {
    state.wsAdapter.disconnect();
    state.wsAdapter = null;
  }

  // Clear repo
  state.repo = null;
  state.indexHandle = null;

  onConnectionChange?.(false);
}

/**
 * Get the current content of a file
 */
export function getFileContent(path: string): string | null {
  const handle = state.fileHandles.get(path);
  if (!handle) return null;
  const doc = handle.doc();
  if (!doc) return null;
  return doc.text;
}

/**
 * Update the content of a file using incremental text updates.
 * This generates granular patches that preserve cursor position on remote clients.
 */
export function updateFileContent(path: string, content: string): void {
  const handle = state.fileHandles.get(path);
  if (!handle) {
    console.warn(`No handle found for file: ${path}`);
    return;
  }

  handle.change(doc => {
    // Use updateText to compute diff and apply incremental changes.
    // This generates proper splice/del patches instead of full replacement.
    updateText(doc, ['text'], content);
  });

  // Update VFS
  vfsAddFile(path, content);
}

/**
 * Create a new file in the project
 */
export async function createFile(path: string, content: string = ''): Promise<void> {
  if (!state.repo || !state.indexHandle) {
    throw new Error('Not connected');
  }

  // Create new document for the file
  const handle = state.repo.create<FileDocument>();
  handle.change(doc => {
    doc.text = content;
  });

  // Add to index
  const indexHandle = state.indexHandle;
  indexHandle.change(doc => {
    doc.files[path] = handle.documentId;
  });

  // Set up subscription
  await subscribeToFile(path, handle);

  // Update VFS
  vfsAddFile(path, content);
}

/**
 * Delete a file from the project
 */
export function deleteFile(path: string): void {
  if (!state.indexHandle) {
    throw new Error('Not connected');
  }

  // Remove from index
  const indexHandle = state.indexHandle;
  indexHandle.change(doc => {
    delete doc.files[path];
  });

  // Remove handle (cleanup will be handled by syncVfsWithFiles on next change event)
  state.fileHandles.delete(path);

  // Update VFS
  vfsRemoveFile(path);
}

/**
 * Check if connected
 */
export function isConnected(): boolean {
  return state.repo !== null && state.indexHandle !== null;
}

// Helper functions

function getFilesFromIndex(doc: IndexDocument): FileEntry[] {
  const files = doc.files || {};
  console.log('Raw files from index:', files);
  return Object.entries(files).map(([path, docId]) => {
    // Convert to string in case it's an automerge type
    const docIdStr = String(docId);
    console.log(`  ${path} -> ${docIdStr} (type: ${typeof docId})`);
    return {
      path,
      docId: docIdStr,
    };
  });
}

async function loadFileDocuments(files: FileEntry[]): Promise<void> {
  if (!state.repo) return;

  for (const file of files) {
    // Ensure the document ID has the automerge: prefix
    const docId = file.docId.startsWith('automerge:')
      ? file.docId
      : `automerge:${file.docId}`;
    console.log(`Loading file document: ${file.path} -> ${docId}`);
    const handle = await state.repo.find<FileDocument>(docId as DocumentId);
    await subscribeToFile(file.path, handle);
  }
}

async function subscribeToFile(path: string, handle: DocHandle<FileDocument>): Promise<void> {
  await handle.whenReady();

  // Store handle
  state.fileHandles.set(path, handle);

  // Initial VFS population (no patches on initial load)
  const doc = handle.doc();
  if (doc) {
    vfsAddFile(path, doc.text || '');
    onFileContent?.(path, doc.text || '', []);
  }

  // Subscribe to changes - forward patches from the change event
  const changeHandler = ({ patches }: { patches: Patch[] }) => {
    const changedDoc = handle.doc();
    if (changedDoc) {
      vfsAddFile(path, changedDoc.text || '');
      onFileContent?.(path, changedDoc.text || '', patches);
    }
  };
  handle.on('change', changeHandler);
  state.cleanupFns.push(() => handle.off('change', changeHandler));
}

async function syncVfsWithFiles(newFiles: FileEntry[]): Promise<void> {
  const newPaths = new Set(newFiles.map(f => f.path));
  const currentPaths = new Set(state.fileHandles.keys());

  // Find new files
  for (const file of newFiles) {
    if (!currentPaths.has(file.path) && state.repo) {
      // Ensure the document ID has the automerge: prefix
      const docId = file.docId.startsWith('automerge:')
        ? file.docId
        : `automerge:${file.docId}`;
      const handle = await state.repo.find<FileDocument>(docId as DocumentId);
      await subscribeToFile(file.path, handle);
    }
  }

  // Find removed files
  for (const path of currentPaths) {
    if (!newPaths.has(path)) {
      state.fileHandles.delete(path);
      vfsRemoveFile(path);
    }
  }
}
