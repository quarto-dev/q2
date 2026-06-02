/**
 * @quarto/quarto-sync-client
 *
 * Automerge sync client for Quarto collaborative documents.
 * Provides real-time document synchronization with a callback-based API.
 */

// Re-export schema types for convenience
export type {
  IndexDocument,
  ActorIdentity,
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
  EditorContentChange,
  TextFilePayload,
  BinaryFilePayload,
  FilePayload,
  SyncClientCallbacks,
  ASTOptions,
  CaptureRef,
  CreateBinaryFileResult,
  CreateProjectOptions,
  CreateProjectResult,
  SyncClientAuthOptions,
} from './types.js';

// Export sync client
export { createSyncClient } from './client.js';
export type { SyncClient } from './client.js';

// Export Node-only adapter (consumers wanting Bearer-authenticated
// WebSocket upgrades). Browser bundles that never touch this module
// will not pull in `ws`.
export {
  NodeWebSocketClientAdapter,
  redactAuthorization,
} from './NodeWebSocketClientAdapter.js';
export type {
  NodeWebSocketClientAdapterOptions,
  WebSocketFactory,
  WebSocketLike,
} from './NodeWebSocketClientAdapter.js';

// Export utilities
export { computeSHA256 } from './hash.js';
export { exportProjectAsZip } from './export-zip.js';
export { parseProjectZip } from './import-zip.js';

// Export replay API
export { createReplaySession } from './replay.js';
export type { ReplaySession, ChangeMetadata } from './replay.js';
