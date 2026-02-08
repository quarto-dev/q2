/**
 * Type definitions for the sync client.
 */

import type { Patch } from '@automerge/automerge-repo';
import type { FileEntry } from '@quarto/quarto-automerge-schema';

// Re-export Patch for consumers
export type { Patch };

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

  /**
   * Incrementally write a modified AST back to QMD text, preserving unchanged
   * portions of the original source text verbatim. Falls back to `writeQmd`
   * if not provided or if the original source is not cached.
   *
   * @param originalQmd - The original QMD source text
   * @param newAst - The modified AST to write
   * @returns The new QMD text with unchanged portions preserved
   */
  incrementalWriteQmd?: (originalQmd: string, newAst: unknown) => string;

  /**
   * Filter which files should be parsed.
   * Defaults to matching .qmd files only if not provided.
   */
  fileFilter?: (path: string) => boolean;
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
