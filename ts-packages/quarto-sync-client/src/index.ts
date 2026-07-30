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
  AnnotatedFileEntry,
  Patch,
  EditorContentChange,
  TextFilePayload,
  BinaryFilePayload,
  FilePayload,
  SyncClientCallbacks,
  ASTOptions,
  CaptureRef,
  ConnectOptions,
  CreateBinaryFileResult,
  CreateProjectOptions,
  CreateProjectResult,
  DisconnectOptions,
  DisconnectReport,
  FindDocRetryOptions,
  StorageKind,
  SyncClientAuthOptions,
  UndeliveredDoc,
} from './types.js';

// Export sync client
export {
  createSyncClient,
  PeerUnavailableError,
  // Locked unavailability wording (bd-vm5e5u10) — reused by hub-mcp
  // so per-file tool errors match the sync client's diagnostics.
  fileUnavailableMessage,
  indexUnavailableMessage,
} from './client.js';
export type {
  SyncClient,
  SyncDiagnostics,
  StrandedFileDiagnostic,
  DocInventoryEntry,
} from './client.js';

// Injectable diagnostic-log sink (bd-sl4o01y0): stdio hosts (hub-mcp)
// must route library diagnostics to stderr; browsers keep console.log.
export { setSyncLogger, syncLog } from './log.js';
export type { SyncLogger } from './log.js';

// Browser adapter with terminal disconnect (zombie-reconnect fix,
// bd-jit6pdwq) — used internally by connect(); exported for tests
// and for consumers that build adapters directly.
export { StoppableWebSocketClientAdapter } from './StoppableWebSocketClientAdapter.js';

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

// In-memory storage adapter. Exported for test hubs that need a
// storageId in their handshake metadata (the real samod hub always
// announces one — exit-drain delivery confirmation keys off it,
// bd-10deu8h4).
export { MemoryStorageAdapter } from './storage-adapter.js';

// Export utilities
export { computeSHA256 } from './hash.js';
export { exportProjectAsZip } from './export-zip.js';
export { parseProjectZip } from './import-zip.js';
export { projectFolderName } from './project-folder-name.js';

// Export replay API
export { createReplaySession } from './replay.js';
export type { ReplaySession, ChangeMetadata } from './replay.js';
