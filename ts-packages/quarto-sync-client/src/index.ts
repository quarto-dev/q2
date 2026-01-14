/**
 * @quarto/quarto-sync-client
 *
 * Automerge sync client for Quarto collaborative documents.
 * Provides real-time document synchronization with a callback-based API.
 */

// Re-export schema types for convenience
export type {
  IndexDocument,
  TextDocumentContent,
  BinaryDocumentContent,
  FileDocumentContent,
  DocumentType,
  FileEntry,
} from '@quarto/quarto-automerge-schema';

export {
  isTextDocument,
  isBinaryDocument,
  getDocumentType,
  getFileExtension,
  isBinaryExtension,
  isTextExtension,
  inferMimeType,
} from '@quarto/quarto-automerge-schema';

// Export sync client types
export type {
  Patch,
  TextFilePayload,
  BinaryFilePayload,
  FilePayload,
  SyncClientCallbacks,
  CreateBinaryFileResult,
  CreateProjectOptions,
  CreateProjectResult,
} from './types.js';

// Export sync client
export { createSyncClient } from './client.js';
export type { SyncClient } from './client.js';

// Export utilities
export { computeSHA256 } from './hash.js';
