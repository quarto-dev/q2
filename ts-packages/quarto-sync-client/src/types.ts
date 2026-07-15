/**
 * Type definitions for the sync client.
 */

import type { Patch } from '@automerge/automerge-repo';
import type { FileEntry, ActorIdentity, CaptureRef } from '@quarto/quarto-automerge-schema';

// Re-export so consumers don't need a second import for the capture sidecar shape
export type { CaptureRef };

// Re-export Patch for consumers
export type { Patch };

// ============================================================================
// Editor Operation Types
// ============================================================================

/**
 * Mirrors Monaco's `IModelContentChange` shape for positional text operations.
 * Used by `applyEditorOperations` to apply splice-based edits to Automerge.
 *
 * `rangeOffset` and `rangeLength` are in UTF-16 code units (JavaScript's native
 * string encoding), matching both Monaco and Automerge's WASM build.
 */
export interface EditorContentChange {
  /** The offset of the range that got replaced (UTF-16 code units). */
  rangeOffset: number;
  /** The length of the range that got replaced (UTF-16 code units). */
  rangeLength: number;
  /** The new text for the range. */
  text: string;
}

// ============================================================================
// File Payload Types (discriminated union)
// ============================================================================

/**
 * Payload for text file content.
 */
export interface TextFilePayload {
  type: 'text';
  text: string;
}

/**
 * Payload for binary file content.
 */
export interface BinaryFilePayload {
  type: 'binary';
  data: Uint8Array;
  mimeType: string;
}

/**
 * Discriminated union for file content.
 * Uses "make illegal states unrepresentable" pattern.
 */
export type FilePayload = TextFilePayload | BinaryFilePayload;

// ============================================================================
// File Entry Annotation
// ============================================================================

/**
 * A {@link FileEntry} annotated with an availability marker at the
 * sync-client boundary (bd-vm5e5u10). This is client-side presentation
 * only — the marker is never written to the index document.
 *
 * `status` is `'unavailable'` when the index references this path but
 * the file's automerge document could not be fetched from the sync
 * server (a "dangling entry"): the file is listed but has no content.
 * Files that loaded normally carry `'ok'` or no marker at all.
 */
export interface AnnotatedFileEntry extends FileEntry {
  status?: 'ok' | 'unavailable';
}

// ============================================================================
// Callback Types
// ============================================================================

/**
 * Callbacks for sync client events.
 * The sync client will invoke these when document state changes.
 */
export interface SyncClientCallbacks {
  /**
   * Called when a new file is added (either loaded or created remotely).
   * Uses discriminated union to ensure type safety.
   */
  onFileAdded: (path: string, file: FilePayload) => void;

  /**
   * Called when a text file's content changes.
   * Includes Automerge patches for incremental updates.
   */
  onFileChanged: (path: string, text: string, patches: Patch[]) => void;

  /**
   * Called when a binary file's content changes.
   */
  onBinaryChanged: (path: string, data: Uint8Array, mimeType: string) => void;

  /**
   * Called when a file is removed.
   */
  onFileRemoved: (path: string) => void;

  /**
   * Called when an index entry references a document that cannot be
   * fetched from the sync server — a "dangling entry" (optional,
   * bd-vm5e5u10). The file is skipped, not fatal: it appears in
   * listings with `status: 'unavailable'` (see
   * {@link AnnotatedFileEntry}) and `onFileAdded` does NOT fire for
   * it. `docId` is the document id exactly as stored in the index.
   * UIs can use this to show a degraded marker.
   */
  onFileUnavailable?: (path: string, docId: string) => void;

  /**
   * Called when the file index changes (optional).
   * Provides the full list of files in the project.
   */
  onFilesChange?: (files: FileEntry[]) => void;

  /**
   * Called when the identity map changes (optional).
   * Provides the full actorId -> ActorIdentity mapping.
   */
  onIdentitiesChange?: (identities: Record<string, ActorIdentity>) => void;

  /**
   * Called when the capture sidecar map changes (optional).
   * Provides the full path -> CaptureRef mapping from the IndexDocument.
   * Fires on initial sync and on every index doc change where the
   * sidecar differs (by JSON-equality) from the last-fired snapshot.
   *
   * Wired in Phase C.3; consumed by Phase C.4 (browser-side replay) to
   * pick up captures the server has eagerly recorded.
   */
  onCapturesChange?: (captures: Record<string, CaptureRef>) => void;

  /**
   * Called when connection state changes (optional).
   */
  onConnectionChange?: (connected: boolean) => void;

  /**
   * Called when an error occurs (optional).
   */
  onError?: (error: Error) => void;

  /**
   * Called when a QMD file's AST changes (optional).
   * Only fired when `astOptions` is provided to `createSyncClient`.
   * The AST is the result of parsing the file with the provided `parseQmd` function.
   * Only called on successful parses — parse failures are logged via console.warn.
   */
  onASTChanged?: (path: string, ast: unknown) => void;
}

// ============================================================================
// AST Options
// ============================================================================

/**
 * Options for AST-level document synchronization.
 *
 * When provided, the sync client will automatically parse QMD files on change
 * and expose an `updateFileAst` function for writing ASTs back to documents.
 *
 * The AST type is opaque to the sync client. Consumers provide parser/writer
 * functions that work with their chosen AST type (e.g., RustQmdJson from
 * @quarto/annotated-qmd).
 */
export interface ASTOptions {
  /**
   * Parse QMD text content into an AST.
   * Return null if parsing fails (the sync client will console.warn and skip).
   */
  parseQmd: (content: string) => unknown;

  /**
   * Convert an AST back to QMD text content.
   */
  writeQmd: (ast: unknown) => string;

  /** Incrementally write a modified AST back to QMD, preserving unchanged source text. */
  incrementalWriteQmd?: (originalQmd: string, newAst: unknown) => string;

  /**
   * Filter which files should be parsed.
   * Defaults to matching .qmd files only if not provided.
   */
  fileFilter?: (path: string) => boolean;
}

// ============================================================================
// Auth Options
// ============================================================================

/**
 * Bearer-auth options for the sync client's WebSocket upgrade.
 *
 * `getBearer` is a getter (not a static string) so the retry loop sees
 * a freshly-refreshed token after the connection-manager's refresh-on-
 * 401 path runs. Presence of this field selects the Node WebSocket
 * adapter; absence selects the browser adapter unchanged.
 */
export interface SyncClientAuthOptions {
  getBearer: () => Promise<string>;
}

/**
 * Which storage adapter backs the automerge `Repo` for a connection.
 *
 * - `'indexeddb'` (default): document caches survive page reloads.
 *   Right for hub-client, whose offline-first flow reads cached docs.
 * - `'memory'`: process-local only. Right for the q2-preview SPA,
 *   whose hubs are ephemeral (the cache can never hit) — and where the
 *   IndexedDB open would otherwise sit on the critical path of the
 *   WebSocket `join` (automerge-repo defers `adapter.connect()` on
 *   `storageSubsystem.id()`; see bd-jit6pdwq research note).
 */
export type StorageKind = 'indexeddb' | 'memory';

/**
 * Retry policy for `repo.find()` calls that lose the cold-start sync
 * race (handle resolves "unavailable" because the doc hasn't synced
 * yet). Retries only happen while at least one peer is connected —
 * with no peers, "unavailable" is the truth, not a race, and
 * hub-client's offline fast-fail must be preserved.
 */
export interface FindDocRetryOptions {
  /** Additional attempts after the first failure. Default 3. */
  attempts?: number;
  /** Base backoff delay; doubles per attempt. Default 250 ms. */
  baseDelayMs?: number;
}

/**
 * Options bag for `connect()` (bd-jit6pdwq). Passed in place of the
 * legacy positional `peerTimeoutMs` number; all fields optional and
 * defaults preserve the legacy behavior exactly.
 */
export interface ConnectOptions {
  /**
   * How long to wait for the samod `peer` event before falling
   * through to offline mode. Defaults to 1 ms (hub-client's
   * probe-then-use-IndexedDB behavior). `Infinity` is supported and
   * means "never fall through": wait for the peer event indefinitely
   * and never enter offline mode. Callers using `Infinity` own the
   * decision of when the server is actually gone (the q2-preview SPA
   * arbitrates via HTTP `/health`, which Firefox's per-IP WebSocket
   * handshake queue cannot stall).
   */
  peerTimeoutMs?: number;
  /**
   * Fail instead of falling back to offline mode when the peer wait
   * times out (bd-xnmd5ni1): `connect` then rejects with
   * `PeerUnavailableError`. For server-backed callers with memory
   * storage (the hub MCP server), offline mode persists nothing —
   * silent fallback is a data black hole. Default false (browser
   * offline-first behavior unchanged).
   */
  requireOnline?: boolean;
  /** Storage backing for this connection. Default `'indexeddb'`. */
  storage?: StorageKind;
  /**
   * WebSocket adapter retry interval (ms). Default: upstream's 5000.
   * Note for Firefox-sensitive callers: do not shorten this — the
   * upstream retry abandons CONNECTING sockets without closing them,
   * and abandoned sockets occupy Firefox's per-IP admission queue
   * until the open timeout. Raise it (or leave the default) and drive
   * reconnects at the application layer instead.
   */
  retryIntervalMs?: number;
  /** Cold-start "unavailable" retry policy. See {@link FindDocRetryOptions}. */
  findDocRetry?: FindDocRetryOptions;
  /** Bearer-auth options (alternative to the positional `auth` param). */
  auth?: SyncClientAuthOptions;
}

// ============================================================================
// Disconnect / exit-drain types (bd-10deu8h4)
// ============================================================================

/**
 * Options bag for `disconnect()`.
 */
export interface DisconnectOptions {
  /**
   * Bounded budget (ms) to drain outbound document sync before tearing
   * the connection down. The drain returns early the moment the
   * connected hub confirms it holds our heads for every tracked
   * document (index + files), and never blocks past the budget.
   *
   * Default `0`: no drain — the existing teardown behavior. Browser
   * callers (hub-client) should keep the default: their IndexedDB
   * storage persists local changes across disconnects, so a blocking
   * drain on tab/component teardown buys nothing. Memory-storage
   * callers whose process is about to exit (the hub MCP server) are
   * the intended users: for them, undelivered == lost (bd-10deu8h4,
   * the 2026-06-12 incident).
   */
  drainMs?: number;
}

/**
 * A document that may not have reached the sync server when the drain
 * budget expired.
 */
export interface UndeliveredDoc {
  /** Project-relative path; `null` for the project's index document. */
  path: string | null;
  /** The automerge document id (bare, no `automerge:` prefix). */
  docId: string;
}

/**
 * Result of `disconnect()`. Only meaningful when a drain was requested
 * (`drainMs > 0`); the default no-drain path always reports
 * `{ drained: true, undelivered: [] }` without checking.
 */
export interface DisconnectReport {
  /**
   * False iff the drain budget expired while at least one tracked
   * document's heads were not yet confirmed by a storage-backed peer.
   */
  drained: boolean;
  /** The documents that were not confirmed delivered. */
  undelivered: UndeliveredDoc[];
}

// ============================================================================
// Result Types
// ============================================================================

/**
 * Result of creating a binary file.
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
 * Options for creating a new project.
 */
export interface CreateProjectOptions {
  /**
   * Sync server URL. Omit (or pass an empty string) to create a
   * **local-only** project: a storage-only Repo with no network adapter,
   * living entirely in the local cache. A hub connection is required only
   * when the project is later opened against a server (bd-e2qnvb4a).
   */
  syncServer?: string;
  /** List of files to create in the project */
  files: Array<{
    path: string;
    content: string;
    contentType: 'text' | 'binary';
    mimeType?: string;
  }>;
  /**
   * Bearer-auth options. When set, the Node WebSocket adapter is used
   * and the upgrade request carries `Authorization: Bearer <token>`.
   * When unset, the browser adapter is used unchanged.
   */
  auth?: SyncClientAuthOptions;
  /**
   * How long to wait for the samod `peer` event before falling through
   * to offline mode (ms). Defaults to 1 ms, which triggers offline mode
   * immediately and lets documents sync to the server in the background.
   *
   * Test helpers like `createProjectOnServer` that need documents to be
   * ONLINE on the hub before returning should pass a longer value (e.g.
   * 10 000 ms) so documents are created while the WebSocket peer is
   * already connected and flush to the server immediately — eliminating
   * the background-sync race against `waitForServerDocuments`.
   */
  peerTimeoutMs?: number;
  /**
   * Fail instead of creating the project in offline mode when the
   * peer wait times out — rejects with `PeerUnavailableError`. See
   * {@link ConnectOptions.requireOnline} (bd-xnmd5ni1).
   */
  requireOnline?: boolean;
  /** Storage backing for this connection. Default `'indexeddb'`. */
  storage?: StorageKind;
  /** WebSocket adapter retry interval (ms). See {@link ConnectOptions}. */
  retryIntervalMs?: number;
  /** Cold-start "unavailable" retry policy for post-create index syncs. */
  findDocRetry?: FindDocRetryOptions;
}

/**
 * Result of creating a new project.
 */
export interface CreateProjectResult {
  /** The document ID of the new IndexDocument */
  indexDocId: string;
  /** List of created files */
  files: FileEntry[];
}
