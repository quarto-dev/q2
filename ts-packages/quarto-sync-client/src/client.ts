/**
 * Sync Client Implementation
 *
 * Manages real-time document synchronization using automerge-repo.
 * Uses callbacks to notify consumers of document changes, allowing
 * them to provide their own storage/VFS implementation.
 */

import { Repo, DocHandle, updateText } from '@automerge/automerge-repo';
import type { DocumentId, Patch } from '@automerge/automerge-repo';
import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';

import type {
  IndexDocument,
  TextDocumentContent,
  BinaryDocumentContent,
  FileEntry,
} from '@quarto/quarto-automerge-schema';
import {
  isTextDocument,
  isBinaryDocument,
  getDocumentType,
  isBinaryExtension,
} from '@quarto/quarto-automerge-schema';

import type {
  SyncClientCallbacks,
  ASTOptions,
  CreateBinaryFileResult,
  CreateProjectOptions,
  CreateProjectResult,
} from './types.js';
import { computeSHA256 } from './hash.js';

// FileDocument can be text or binary - use runtime detection
type FileDocument = TextDocumentContent | BinaryDocumentContent;

/**
 * Internal state for a sync client instance.
 */
interface SyncClientState {
  repo: Repo | null;
  wsAdapter: BrowserWebSocketClientAdapter | null;
  indexHandle: DocHandle<IndexDocument> | null;
  fileHandles: Map<string, DocHandle<FileDocument>>;
  binaryFiles: Set<string>;
  cleanupFns: (() => void)[];
}

/**
 * Default file filter: only parse .qmd files.
 */
function defaultFileFilter(path: string): boolean {
  return path.endsWith('.qmd');
}

/**
 * Create a new sync client with the given callbacks.
 *
 * @param callbacks - Callbacks for sync client events
 * @param astOptions - Optional AST options for automatic parsing of QMD files.
 *   When provided, the sync client will parse text files on change and fire
 *   `onASTChanged` on successful parses. Also enables `updateFileAst`.
 */
export function createSyncClient(callbacks: SyncClientCallbacks, astOptions?: ASTOptions) {
  const state: SyncClientState = {
    repo: null,
    wsAdapter: null,
    indexHandle: null,
    fileHandles: new Map(),
    binaryFiles: new Set(),
    cleanupFns: [],
  };

  // AST cache: last successful parse per file (for round-tripping)
  const astCache = new Map<string, { source: string; ast: unknown }>();

  // Resolved file filter
  const astFileFilter = astOptions?.fileFilter ?? defaultFileFilter;

  /**
   * Try parsing a text file and fire onASTChanged if successful.
   * Parse failures (null return) are logged. Exceptions from the parser
   * are caught and logged; exceptions from the callback are NOT caught.
   */
  function tryParseAndNotify(path: string, text: string): void {
    if (!astOptions || !callbacks.onASTChanged) return;
    if (!astFileFilter(path)) return;

    // Parse the text — catch exceptions from the parser only
    let ast: unknown;
    try {
      ast = astOptions.parseQmd(text);
    } catch (e) {
      console.warn(`[quarto-sync-client] Parse threw for ${path}:`, e);
      return;
    }

    if (ast == null) {
      console.warn(`[quarto-sync-client] Parse returned null for ${path}`);
      return;
    }

    // Cache and notify — exceptions from callback propagate normally
    astCache.set(path, { source: text, ast });
    callbacks.onASTChanged(path, ast);
  }

  // Helper: get files from index document
  function getFilesFromIndex(doc: IndexDocument): FileEntry[] {
    const files = doc.files || {};
    return Object.entries(files).map(([path, docId]) => ({
      path,
      docId: String(docId),
    }));
  }

  // Helper: wait for peer connection
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
        repo.networkSubsystem.off('peer', onPeer);
      };

      repo.networkSubsystem.on('peer', onPeer);
    });
  }

  // Helper: subscribe to a file document
  async function subscribeToFile(path: string, handle: DocHandle<FileDocument>): Promise<void> {
    await handle.whenReady();
    state.fileHandles.set(path, handle);

    const doc = handle.doc();
    if (doc) {
      const docType = getDocumentType(doc);

      if (docType === 'binary' && isBinaryDocument(doc)) {
        state.binaryFiles.add(path);
        callbacks.onFileAdded(path, {
          type: 'binary',
          data: doc.content,
          mimeType: doc.mimeType,
        });
      } else if (docType === 'text' && isTextDocument(doc)) {
        const text = doc.text || '';
        callbacks.onFileAdded(path, {
          type: 'text',
          text,
        });
        tryParseAndNotify(path, text);
      } else {
        // Invalid or unknown document type - try to infer from extension
        if (isBinaryExtension(path)) {
          console.warn(`Document at ${path} has invalid structure but binary extension, skipping`);
        } else {
          console.warn(`Document at ${path} has invalid structure, treating as empty text`);
          callbacks.onFileAdded(path, { type: 'text', text: '' });
        }
      }
    }

    // Subscribe to changes
    const changeHandler = ({ patches }: { patches: Patch[] }) => {
      const changedDoc = handle.doc();
      if (!changedDoc) return;

      const docType = getDocumentType(changedDoc);

      if (docType === 'binary' && isBinaryDocument(changedDoc)) {
        state.binaryFiles.add(path);
        callbacks.onBinaryChanged(path, changedDoc.content, changedDoc.mimeType);
      } else if (docType === 'text' && isTextDocument(changedDoc)) {
        state.binaryFiles.delete(path);
        const text = changedDoc.text || '';
        callbacks.onFileChanged(path, text, patches);
        tryParseAndNotify(path, text);
      }
    };

    handle.on('change', changeHandler);
    state.cleanupFns.push(() => handle.off('change', changeHandler));
  }

  // Helper: subscribe to file (internal, doesn't wait for ready)
  async function subscribeToFileInternal(path: string, handle: DocHandle<FileDocument>): Promise<void> {
    state.fileHandles.set(path, handle);

    const changeHandler = ({ patches }: { patches: Patch[] }) => {
      const changedDoc = handle.doc();
      if (!changedDoc) return;

      const docType = getDocumentType(changedDoc);

      if (docType === 'binary' && isBinaryDocument(changedDoc)) {
        state.binaryFiles.add(path);
        callbacks.onBinaryChanged(path, changedDoc.content, changedDoc.mimeType);
      } else if (docType === 'text' && isTextDocument(changedDoc)) {
        state.binaryFiles.delete(path);
        const text = changedDoc.text || '';
        callbacks.onFileChanged(path, text, patches);
        tryParseAndNotify(path, text);
      }
    };

    handle.on('change', changeHandler);
    state.cleanupFns.push(() => handle.off('change', changeHandler));
  }

  // Helper: load file documents
  async function loadFileDocuments(files: FileEntry[]): Promise<void> {
    if (!state.repo) return;

    for (const file of files) {
      const docId = file.docId.startsWith('automerge:')
        ? file.docId
        : `automerge:${file.docId}`;
      const handle = await state.repo.find<FileDocument>(docId as DocumentId);
      await subscribeToFile(file.path, handle);
    }
  }

  // Helper: sync files with index changes
  async function syncWithFiles(newFiles: FileEntry[]): Promise<void> {
    const newPaths = new Set(newFiles.map(f => f.path));
    const currentPaths = new Set(state.fileHandles.keys());

    // Find new files
    for (const file of newFiles) {
      if (!currentPaths.has(file.path) && state.repo) {
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
        astCache.delete(path);
        callbacks.onFileRemoved(path);
      }
    }
  }

  // ============================================================================
  // Public API
  // ============================================================================

  /**
   * Connect to a sync server and load a project.
   */
  async function connect(syncServerUrl: string, indexDocId: string): Promise<FileEntry[]> {
    // Disconnect from any existing connection
    await disconnect();

    try {
      state.wsAdapter = new BrowserWebSocketClientAdapter(syncServerUrl);
      state.repo = new Repo({ network: [state.wsAdapter] });

      console.log('Waiting for peer connection...');
      await waitForPeer(state.repo, 30000);
      console.log('Peer connected');

      const docId = indexDocId as DocumentId;
      const indexHandle = await state.repo.find<IndexDocument>(docId);
      state.indexHandle = indexHandle;

      await indexHandle.whenReady();

      const doc = indexHandle.doc();
      if (!doc) {
        throw new Error('Failed to load index document');
      }

      const files = getFilesFromIndex(doc);

      // Subscribe to index changes
      const indexChangeHandler = () => {
        const changedDoc = indexHandle.doc();
        if (changedDoc) {
          const newFiles = getFilesFromIndex(changedDoc);
          syncWithFiles(newFiles);
          callbacks.onFilesChange?.(newFiles);
        }
      };
      indexHandle.on('change', indexChangeHandler);
      state.cleanupFns.push(() => indexHandle.off('change', indexChangeHandler));

      // Load file documents
      await loadFileDocuments(files);

      callbacks.onConnectionChange?.(true);
      return files;
    } catch (err) {
      const error = err instanceof Error ? err : new Error(String(err));
      callbacks.onError?.(error);
      throw error;
    }
  }

  /**
   * Disconnect from the sync server.
   */
  async function disconnect(): Promise<void> {
    // Clean up subscriptions
    for (const cleanup of state.cleanupFns) {
      cleanup();
    }
    state.cleanupFns = [];

    // Notify about removed files
    for (const path of state.fileHandles.keys()) {
      callbacks.onFileRemoved(path);
    }

    state.fileHandles.clear();
    state.binaryFiles.clear();
    astCache.clear();

    if (state.wsAdapter) {
      state.wsAdapter.disconnect();
      state.wsAdapter = null;
    }

    state.repo = null;
    state.indexHandle = null;

    callbacks.onConnectionChange?.(false);
  }

  /**
   * Check if a file is binary.
   */
  function isFileBinary(path: string): boolean {
    return state.binaryFiles.has(path);
  }

  /**
   * Get text content of a file.
   */
  function getFileContent(path: string): string | null {
    const handle = state.fileHandles.get(path);
    if (!handle) return null;
    if (state.binaryFiles.has(path)) return null;

    const doc = handle.doc();
    if (!doc || !isTextDocument(doc)) return null;
    return doc.text;
  }

  /**
   * Get binary content of a file.
   */
  function getBinaryFileContent(path: string): { content: Uint8Array; mimeType: string } | null {
    const handle = state.fileHandles.get(path);
    if (!handle) return null;
    if (!state.binaryFiles.has(path)) return null;

    const doc = handle.doc();
    if (!doc || !isBinaryDocument(doc)) return null;
    return { content: doc.content, mimeType: doc.mimeType };
  }

  /**
   * Update text file content using incremental updates.
   */
  function updateFileContent(path: string, content: string): void {
    const handle = state.fileHandles.get(path);
    if (!handle) {
      console.warn(`No handle found for file: ${path}`);
      return;
    }

    handle.change(doc => {
      updateText(doc, ['text'], content);
    });

    // Notify callback (local change)
    callbacks.onFileChanged(path, content, []);
    tryParseAndNotify(path, content);
  }

  /**
   * Create a new text file.
   */
  async function createFile(path: string, content: string = ''): Promise<void> {
    if (!state.repo || !state.indexHandle) {
      throw new Error('Not connected');
    }

    const handle = state.repo.create<TextDocumentContent>();
    handle.change(doc => {
      doc.text = content;
    });

    const indexHandle = state.indexHandle;
    indexHandle.change(doc => {
      doc.files[path] = handle.documentId;
    });

    await subscribeToFileInternal(path, handle as unknown as DocHandle<FileDocument>);
    callbacks.onFileAdded(path, { type: 'text', text: content });
    tryParseAndNotify(path, content);
  }

  /**
   * Create a new binary file with deduplication.
   */
  async function createBinaryFile(
    path: string,
    content: Uint8Array,
    mimeType: string
  ): Promise<CreateBinaryFileResult> {
    if (!state.repo || !state.indexHandle) {
      throw new Error('Not connected');
    }

    const hash = await computeSHA256(content);
    const indexDoc = state.indexHandle.doc();
    const existingDocId = indexDoc?.files?.[path];

    if (existingDocId) {
      const existingHandle = state.fileHandles.get(path);
      if (existingHandle) {
        const existingDoc = existingHandle.doc();
        if (existingDoc && isBinaryDocument(existingDoc) && existingDoc.hash === hash) {
          return { docId: existingDocId, path, deduplicated: true };
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
    }

    const handle = state.repo.create<BinaryDocumentContent>();
    handle.change(doc => {
      doc.content = content;
      doc.mimeType = mimeType;
      doc.hash = hash;
    });

    const indexHandle = state.indexHandle;
    const docId = handle.documentId;
    indexHandle.change(doc => {
      doc.files[path] = docId;
    });

    state.binaryFiles.add(path);
    await subscribeToFileInternal(path, handle as unknown as DocHandle<FileDocument>);
    callbacks.onFileAdded(path, { type: 'binary', data: content, mimeType });

    return { docId, path, deduplicated: false };
  }

  /**
   * Delete a file.
   */
  function deleteFile(path: string): void {
    if (!state.indexHandle) {
      throw new Error('Not connected');
    }

    const indexHandle = state.indexHandle;
    indexHandle.change(doc => {
      delete doc.files[path];
    });

    state.fileHandles.delete(path);
    state.binaryFiles.delete(path);
    astCache.delete(path);
    callbacks.onFileRemoved(path);
  }

  /**
   * Rename a file.
   */
  function renameFile(oldPath: string, newPath: string): void {
    if (!state.indexHandle) {
      throw new Error('Not connected');
    }

    const indexDoc = state.indexHandle.doc();
    const docId = indexDoc?.files?.[oldPath];
    if (!docId) {
      throw new Error(`File not found: ${oldPath}`);
    }

    if (indexDoc?.files?.[newPath]) {
      throw new Error(`File already exists: ${newPath}`);
    }

    const indexHandle = state.indexHandle;
    indexHandle.change(doc => {
      delete doc.files[oldPath];
      doc.files[newPath] = docId;
    });

    const handle = state.fileHandles.get(oldPath);
    if (handle) {
      state.fileHandles.delete(oldPath);
      state.fileHandles.set(newPath, handle);
    }

    if (state.binaryFiles.has(oldPath)) {
      state.binaryFiles.delete(oldPath);
      state.binaryFiles.add(newPath);
    }

    // Notify callbacks
    callbacks.onFileRemoved(oldPath);
    const content = getFileContent(newPath);
    const binary = getBinaryFileContent(newPath);
    if (binary) {
      callbacks.onFileAdded(newPath, { type: 'binary', data: binary.content, mimeType: binary.mimeType });
    } else if (content !== null) {
      callbacks.onFileAdded(newPath, { type: 'text', text: content });
    }
  }

  /**
   * Check if connected.
   */
  function isConnected(): boolean {
    return state.repo !== null && state.indexHandle !== null;
  }

  /**
   * Get file handle for presence/ephemeral messaging.
   */
  function getFileHandle(path: string): DocHandle<FileDocument> | null {
    return state.fileHandles.get(path) ?? null;
  }

  /**
   * Get all file paths.
   */
  function getFilePaths(): string[] {
    return Array.from(state.fileHandles.keys());
  }

  /**
   * Create a new project with the given files.
   */
  async function createNewProject(options: CreateProjectOptions): Promise<CreateProjectResult> {
    await disconnect();

    try {
      state.wsAdapter = new BrowserWebSocketClientAdapter(options.syncServer);
      state.repo = new Repo({ network: [state.wsAdapter] });

      await waitForPeer(state.repo, 30000);

      const indexHandle = state.repo.create<IndexDocument>();
      indexHandle.change(doc => {
        doc.files = {};
      });
      state.indexHandle = indexHandle;

      const indexDocId = indexHandle.documentId;
      const createdFiles: FileEntry[] = [];

      for (const file of options.files) {
        if (file.contentType === 'binary') {
          const binaryContent = Uint8Array.from(atob(file.content), c => c.charCodeAt(0));
          const mimeType = file.mimeType || 'application/octet-stream';
          const hash = await computeSHA256(binaryContent);

          const handle = state.repo.create<BinaryDocumentContent>();
          handle.change(doc => {
            doc.content = binaryContent;
            doc.mimeType = mimeType;
            doc.hash = hash;
          });

          const docId = handle.documentId;
          indexHandle.change(doc => {
            doc.files[file.path] = docId;
          });

          state.binaryFiles.add(file.path);
          await subscribeToFileInternal(file.path, handle as unknown as DocHandle<FileDocument>);
          callbacks.onFileAdded(file.path, { type: 'binary', data: binaryContent, mimeType });
          createdFiles.push({ path: file.path, docId });
        } else {
          const handle = state.repo.create<TextDocumentContent>();
          handle.change(doc => {
            doc.text = file.content;
          });

          const docId = handle.documentId;
          indexHandle.change(doc => {
            doc.files[file.path] = docId;
          });

          await subscribeToFileInternal(file.path, handle as unknown as DocHandle<FileDocument>);
          callbacks.onFileAdded(file.path, { type: 'text', text: file.content });
          createdFiles.push({ path: file.path, docId });
        }
      }

      // Subscribe to index changes
      const indexChangeHandler = () => {
        const changedDoc = indexHandle.doc();
        if (changedDoc) {
          const newFiles = getFilesFromIndex(changedDoc);
          syncWithFiles(newFiles);
          callbacks.onFilesChange?.(newFiles);
        }
      };
      indexHandle.on('change', indexChangeHandler);
      state.cleanupFns.push(() => indexHandle.off('change', indexChangeHandler));

      callbacks.onConnectionChange?.(true);

      return { indexDocId, files: createdFiles };
    } catch (err) {
      const error = err instanceof Error ? err : new Error(String(err));
      callbacks.onError?.(error);
      throw error;
    }
  }

  /**
   * Update a file's content by providing a new AST.
   * The AST is converted to QMD text using the provided writeQmd function,
   * then the text is synced via updateFileContent.
   *
   * Requires astOptions to be provided when creating the sync client.
   * Throws if writeQmd throws or if astOptions was not configured.
   */
  function updateFileAst(path: string, ast: unknown): void {
    if (!astOptions) {
      throw new Error('updateFileAst called without astOptions configured');
    }

    const qmdText = astOptions.writeQmd(ast);
    updateFileContent(path, qmdText);
  }

  /**
   * Get the last successfully parsed AST for a file.
   * Returns null if the file hasn't been parsed or if astOptions is not configured.
   */
  function getFileAst(path: string): unknown {
    return astCache.get(path)?.ast ?? null;
  }

  // Return the public API
  return {
    connect,
    disconnect,
    isFileBinary,
    getFileContent,
    getBinaryFileContent,
    updateFileContent,
    updateFileAst,
    getFileAst,
    createFile,
    createBinaryFile,
    deleteFile,
    renameFile,
    isConnected,
    getFileHandle,
    getFilePaths,
    createNewProject,
  };
}

/**
 * Type for the sync client instance.
 */
export type SyncClient = ReturnType<typeof createSyncClient>;
