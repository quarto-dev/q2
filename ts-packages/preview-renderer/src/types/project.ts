/**
 * Project types for hub-client.
 *
 * Re-exports shared types from @quarto/quarto-automerge-schema
 * and defines application-specific types.
 */

// Re-export all shared types from schema package
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

// ============================================================================
// File Type Helpers
// ============================================================================

/**
 * Check if a file path represents a QMD (Quarto Markdown) file.
 *
 * QMD files receive special treatment in hub-client:
 * - Live preview rendering via WASM
 * - Document outline (symbols)
 * - Code folding for frontmatter, code cells, and sections
 * - Diagnostics from the Quarto parser
 *
 * @param path - File path to check (can be null/undefined)
 * @returns true if the path ends with .qmd (case-insensitive)
 */
export function isQmdFile(path: string | null | undefined): boolean {
  return path?.toLowerCase().endsWith('.qmd') ?? false;
}

/**
 * Check if a file path is a renderable source file (`.qmd` or `.md`).
 *
 * bd-6d2wj4zp Phase 5 (D10/D11): `.md` files are render inputs (opted in
 * via the project render list) and get the same live preview, outline,
 * folding, and diagnostics treatment as `.qmd`. Surfaces gating those
 * features should use this predicate; `isQmdFile` remains for genuinely
 * `.qmd`-only checks.
 *
 * @param path - File path to check (can be null/undefined)
 * @returns true if the path ends with .qmd or .md (case-insensitive)
 */
export function isSourceFile(path: string | null | undefined): boolean {
  const lower = path?.toLowerCase();
  if (!lower) return false;
  return lower.endsWith('.qmd') || lower.endsWith('.md');
}

// ============================================================================
// Application-Specific Types
// ============================================================================

/**
 * Project entry stored in IndexedDB.
 * Contains the connection information for an automerge project.
 */
export interface ProjectEntry {
  id: string;                 // Unique local ID for this entry
  indexDocId: string;         // bs58-encoded automerge DocumentId for IndexDocument
  syncServer: string;         // WebSocket URL for the sync server
  description: string;        // User-provided description
  createdAt: string;          // ISO timestamp when entry was created
  lastAccessed: string;       // ISO timestamp when last accessed
}

/**
 * State for the currently selected project.
 */
export interface ProjectState {
  entry: ProjectEntry;
  files: import('@quarto/quarto-automerge-schema').FileEntry[];
  currentFile: import('@quarto/quarto-automerge-schema').FileEntry | null;
  connected: boolean;
}
