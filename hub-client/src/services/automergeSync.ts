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
import type { FileEntry, TextDocumentContent, BinaryDocumentContent } from '../types/project';
import { isTextDocument, isBinaryDocument, getDocumentType, isBinaryExtension } from '../types/project';
import { vfsAddFile, vfsAddBinaryFile, vfsRemoveFile, vfsClear, initWasm } from './wasmRenderer';
import { computeSHA256 } from './resourceService';

// Document types
interface IndexDocument {
  files: Record<string, string>; // path -> docId mapping
}

// FileDocument can be text or binary - use runtime detection
type FileDocument = TextDocumentContent | BinaryDocumentContent;

// Connection state
interface SyncState {
  repo: Repo | null;
  wsAdapter: BrowserWebSocketClientAdapter | null;
  indexHandle: DocHandle<IndexDocument> | null;
  fileHandles: Map<string, DocHandle<FileDocument>>;
  /** Track which files are binary for quick lookup */
  binaryFiles: Set<string>;
  cleanupFns: (() => void)[];
}

const state: SyncState = {
  repo: null,
  wsAdapter: null,
  indexHandle: null,
  fileHandles: new Map(),
  binaryFiles: new Set(),
  cleanupFns: [],
};

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

/**
 * Set event handlers for sync events
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

    // Wait for a peer to connect before requesting documents
    // This prevents a race condition where the document is marked unavailable
    // before the websocket connection is established
    console.log('Waiting for peer connection...');
    await waitForPeer(state.repo, 30000); // 30 second timeout
    console.log('Peer connected');

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

  // Clear file handles and binary tracking
  state.fileHandles.clear();
  state.binaryFiles.clear();

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
 * Check if a file is binary (based on loaded document type)
 */
export function isFileBinary(path: string): boolean {
  return state.binaryFiles.has(path);
}

/**
 * Get the current text content of a file.
 * Returns null if file doesn't exist or is binary.
 */
export function getFileContent(path: string): string | null {
  const handle = state.fileHandles.get(path);
  if (!handle) return null;
  if (state.binaryFiles.has(path)) return null; // Binary files don't have text content

  const doc = handle.doc();
  if (!doc || !isTextDocument(doc)) return null;
  return doc.text;
}

/**
 * Get the current binary content of a file.
 * Returns null if file doesn't exist or is text.
 */
export function getBinaryFileContent(path: string): { content: Uint8Array; mimeType: string } | null {
  const handle = state.fileHandles.get(path);
  if (!handle) return null;
  if (!state.binaryFiles.has(path)) return null; // Text files don't have binary content

  const doc = handle.doc();
  if (!doc || !isBinaryDocument(doc)) return null;
  return { content: doc.content, mimeType: doc.mimeType };
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
 * Create a new text file in the project
 */
export async function createFile(path: string, content: string = ''): Promise<void> {
  if (!state.repo || !state.indexHandle) {
    throw new Error('Not connected');
  }

  // Create new document for the file (cast to TextDocumentContent for text files)
  const handle = state.repo.create<TextDocumentContent>();
  handle.change(doc => {
    doc.text = content;
  });

  // Add to index
  const indexHandle = state.indexHandle;
  indexHandle.change(doc => {
    doc.files[path] = handle.documentId;
  });

  // Set up subscription (cast back to FileDocument for storage)
  await subscribeToFile(path, handle as unknown as DocHandle<FileDocument>);

  // Update VFS
  vfsAddFile(path, content);
}

/**
 * Result of creating a binary file
 */
export interface CreateBinaryFileResult {
  /** The document ID of the created file */
  docId: string;
  /** The actual path used (may differ from original if conflict was resolved) */
  path: string;
  /** Whether the file was deduplicated (same hash as existing file) */
  deduplicated: boolean;
}

/**
 * Create a new binary file in the project.
 *
 * Handles conflict resolution:
 * - If a file with the same path and same hash exists, returns existing file (deduplication)
 * - If a file with the same path but different hash exists, generates a unique name
 *
 * @param path - Desired file path (may be modified to resolve conflicts)
 * @param content - Binary content as Uint8Array
 * @param mimeType - MIME type of the content
 * @returns Information about the created file
 */
export async function createBinaryFile(
  path: string,
  content: Uint8Array,
  mimeType: string
): Promise<CreateBinaryFileResult> {
  if (!state.repo || !state.indexHandle) {
    throw new Error('Not connected');
  }

  // Compute hash for conflict detection
  const hash = await computeSHA256(content);

  // Check for existing file at this path
  const indexDoc = state.indexHandle.doc();
  const existingDocId = indexDoc?.files?.[path];

  if (existingDocId) {
    // Check if it's the same content (same hash)
    const existingHandle = state.fileHandles.get(path);
    if (existingHandle) {
      const existingDoc = existingHandle.doc();
      if (existingDoc && isBinaryDocument(existingDoc) && existingDoc.hash === hash) {
        // Same content - deduplication
        console.log(`Binary file at ${path} has same content, reusing existing document`);
        return {
          docId: existingDocId,
          path,
          deduplicated: true,
        };
      }
    }

    // Different content - generate unique name
    const lastDot = path.lastIndexOf('.');
    const hashPrefix = hash.slice(0, 8);
    if (lastDot > 0) {
      const name = path.slice(0, lastDot);
      const ext = path.slice(lastDot);
      path = `${name}-${hashPrefix}${ext}`;
    } else {
      path = `${path}-${hashPrefix}`;
    }
    console.log(`Binary file conflict, using unique name: ${path}`);
  }

  // Create new document for the binary file
  const handle = state.repo.create<BinaryDocumentContent>();
  handle.change(doc => {
    doc.content = content;
    doc.mimeType = mimeType;
    doc.hash = hash;
  });

  // Add to index
  const indexHandle = state.indexHandle;
  const docId = handle.documentId;
  indexHandle.change(doc => {
    doc.files[path] = docId;
  });

  // Track as binary and set up subscription
  state.binaryFiles.add(path);
  await subscribeToFile(path, handle as unknown as DocHandle<FileDocument>);

  // Update VFS with binary content
  vfsAddBinaryFile(path, content);

  return {
    docId,
    path,
    deduplicated: false,
  };
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

  // Remove handle and binary tracking
  state.fileHandles.delete(path);
  state.binaryFiles.delete(path);

  // Update VFS
  vfsRemoveFile(path);
}

/**
 * Rename a file in the project.
 * Updates the index mapping without changing the document content.
 */
export function renameFile(oldPath: string, newPath: string): void {
  if (!state.indexHandle) {
    throw new Error('Not connected');
  }

  // Get the document ID from the current index
  const indexDoc = state.indexHandle.doc();
  const docId = indexDoc?.files?.[oldPath];
  if (!docId) {
    throw new Error(`File not found: ${oldPath}`);
  }

  // Check if new path already exists
  if (indexDoc?.files?.[newPath]) {
    throw new Error(`File already exists: ${newPath}`);
  }

  // Update the index: delete old, add new with same docId
  const indexHandle = state.indexHandle;
  indexHandle.change(doc => {
    delete doc.files[oldPath];
    doc.files[newPath] = docId;
  });

  // Update local state
  const handle = state.fileHandles.get(oldPath);
  if (handle) {
    state.fileHandles.delete(oldPath);
    state.fileHandles.set(newPath, handle);
  }

  // Update binary tracking
  if (state.binaryFiles.has(oldPath)) {
    state.binaryFiles.delete(oldPath);
    state.binaryFiles.add(newPath);

    // Update VFS for binary files
    const binaryContent = getBinaryFileContent(newPath);
    if (binaryContent) {
      vfsRemoveFile(oldPath);
      vfsAddBinaryFile(newPath, binaryContent.content);
    }
  } else {
    // Update VFS for text files
    const textContent = getFileContent(newPath);
    if (textContent !== null) {
      vfsRemoveFile(oldPath);
      vfsAddFile(newPath, textContent);
    }
  }
}

/**
 * Check if connected
 */
export function isConnected(): boolean {
  return state.repo !== null && state.indexHandle !== null;
}

/**
 * Options for creating a new project
 */
export interface CreateProjectOptions {
  /** Sync server URL */
  syncServer: string;
  /** List of files to create in the project */
  files: Array<{
    path: string;
    content: string;
    contentType: 'text' | 'binary';
    mimeType?: string;
  }>;
}

/**
 * Result of creating a new project
 */
export interface CreateProjectResult {
  /** The document ID of the new IndexDocument */
  indexDocId: string;
  /** List of created files */
  files: FileEntry[];
}

/**
 * Create a new project with the given files.
 *
 * This creates a new IndexDocument, populates it with file documents,
 * and connects to the sync server. The project can then be saved to
 * IndexedDB using projectStorage.addProject().
 */
export async function createNewProject(options: CreateProjectOptions): Promise<CreateProjectResult> {
  // Ensure WASM is initialized for VFS operations
  await initWasm();

  // Disconnect from any existing connection
  await disconnect();

  try {
    // Create WebSocket adapter
    state.wsAdapter = new BrowserWebSocketClientAdapter(options.syncServer);

    // Create repo with the network adapter
    state.repo = new Repo({
      network: [state.wsAdapter],
    });

    // Wait for peer connection
    console.log('Creating new project, waiting for peer connection...');
    await waitForPeer(state.repo, 30000);
    console.log('Peer connected');

    // Create new IndexDocument
    const indexHandle = state.repo.create<IndexDocument>();
    indexHandle.change(doc => {
      doc.files = {};
    });
    state.indexHandle = indexHandle;

    const indexDocId = indexHandle.documentId;
    console.log('Created new IndexDocument:', indexDocId);

    // Create file documents for each scaffold file
    const createdFiles: FileEntry[] = [];

    for (const file of options.files) {
      if (file.contentType === 'binary') {
        // Decode base64 content for binary files
        const binaryContent = Uint8Array.from(atob(file.content), c => c.charCodeAt(0));
        const mimeType = file.mimeType || 'application/octet-stream';

        // Create binary document
        const handle = state.repo.create<BinaryDocumentContent>();
        const hash = await computeSHA256(binaryContent);
        handle.change(doc => {
          doc.content = binaryContent;
          doc.mimeType = mimeType;
          doc.hash = hash;
        });

        // Add to index
        const docId = handle.documentId;
        indexHandle.change(doc => {
          doc.files[file.path] = docId;
        });

        // Track and subscribe
        state.binaryFiles.add(file.path);
        await subscribeToFileInternal(file.path, handle as unknown as DocHandle<FileDocument>);
        vfsAddBinaryFile(file.path, binaryContent);

        createdFiles.push({ path: file.path, docId });
        console.log(`Created binary file: ${file.path}`);
      } else {
        // Create text document
        const handle = state.repo.create<TextDocumentContent>();
        handle.change(doc => {
          doc.text = file.content;
        });

        // Add to index
        const docId = handle.documentId;
        indexHandle.change(doc => {
          doc.files[file.path] = docId;
        });

        // Subscribe to changes
        await subscribeToFileInternal(file.path, handle as unknown as DocHandle<FileDocument>);
        vfsAddFile(file.path, file.content);

        createdFiles.push({ path: file.path, docId });
        console.log(`Created text file: ${file.path}`);
      }
    }

    // Subscribe to index changes
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

    onConnectionChange?.(true);

    return {
      indexDocId,
      files: createdFiles,
    };
  } catch (err) {
    const error = err instanceof Error ? err : new Error(String(err));
    onError?.(error);
    throw error;
  }
}

/**
 * Internal version of subscribeToFile that doesn't wait for ready
 * (used when we just created the document)
 */
async function subscribeToFileInternal(path: string, handle: DocHandle<FileDocument>): Promise<void> {
  // Store handle
  state.fileHandles.set(path, handle);

  // Subscribe to changes - forward patches from the change event
  const changeHandler = ({ patches }: { patches: Patch[] }) => {
    const changedDoc = handle.doc();
    if (!changedDoc) return;

    const docType = getDocumentType(changedDoc);

    if (docType === 'binary' && isBinaryDocument(changedDoc)) {
      state.binaryFiles.add(path);
      vfsAddBinaryFile(path, changedDoc.content);
      onBinaryContent?.(path, changedDoc.content, changedDoc.mimeType);
    } else if (docType === 'text' && isTextDocument(changedDoc)) {
      state.binaryFiles.delete(path);
      vfsAddFile(path, changedDoc.text || '');
      onFileContent?.(path, changedDoc.text || '', patches);
    }
  };
  handle.on('change', changeHandler);
  state.cleanupFns.push(() => handle.off('change', changeHandler));
}

/**
 * Get the DocHandle for a file by path.
 * Used by the presence service for ephemeral messaging.
 */
export function getFileHandle(path: string): DocHandle<FileDocument> | null {
  return state.fileHandles.get(path) ?? null;
}

/**
 * Get all current file paths that have handles.
 */
export function getFilePaths(): string[] {
  return Array.from(state.fileHandles.keys());
}

// Helper functions

/**
 * Wait for a peer to connect to the repo.
 * This is necessary to avoid a race condition where we try to find documents
 * before the websocket connection is established, causing them to be marked unavailable.
 */
function waitForPeer(repo: Repo, timeoutMs: number = 30000): Promise<void> {
  return new Promise((resolve, reject) => {
    const timeoutId = setTimeout(() => {
      cleanup();
      reject(new Error('Timeout waiting for peer connection'));
    }, timeoutMs);

    const onPeer = () => {
      cleanup();
      resolve();
    };

    const cleanup = () => {
      clearTimeout(timeoutId);
      // Access the networkSubsystem to remove the listener
      // Note: networkSubsystem is marked @hidden but is the only way to listen for peer events
      repo.networkSubsystem.off('peer', onPeer);
    };

    // Listen for peer connection
    repo.networkSubsystem.on('peer', onPeer);
  });
}

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

  // Detect document type and handle accordingly
  const doc = handle.doc();
  if (doc) {
    const docType = getDocumentType(doc);

    if (docType === 'binary' && isBinaryDocument(doc)) {
      // Binary document
      state.binaryFiles.add(path);
      vfsAddBinaryFile(path, doc.content);
      onBinaryContent?.(path, doc.content, doc.mimeType);
    } else if (docType === 'text' && isTextDocument(doc)) {
      // Text document
      vfsAddFile(path, doc.text || '');
      onFileContent?.(path, doc.text || '', []);
    } else {
      // Invalid or unknown document type - try to infer from extension
      if (isBinaryExtension(path)) {
        console.warn(`Document at ${path} has invalid structure but binary extension, skipping`);
      } else {
        console.warn(`Document at ${path} has invalid structure, treating as empty text`);
        vfsAddFile(path, '');
        onFileContent?.(path, '', []);
      }
    }
  }

  // Subscribe to changes - forward patches from the change event
  const changeHandler = ({ patches }: { patches: Patch[] }) => {
    const changedDoc = handle.doc();
    if (!changedDoc) return;

    const docType = getDocumentType(changedDoc);

    if (docType === 'binary' && isBinaryDocument(changedDoc)) {
      // Binary document changed
      state.binaryFiles.add(path);
      vfsAddBinaryFile(path, changedDoc.content);
      onBinaryContent?.(path, changedDoc.content, changedDoc.mimeType);
    } else if (docType === 'text' && isTextDocument(changedDoc)) {
      // Text document changed
      state.binaryFiles.delete(path); // In case it was previously binary
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
      state.binaryFiles.delete(path);
      vfsRemoveFile(path);
    }
  }
}
