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
