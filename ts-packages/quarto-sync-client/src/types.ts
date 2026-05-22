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
  /** Sync server URL */
  syncServer: string;
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
