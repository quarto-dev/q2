/**
 * @quarto/quarto-automerge-schema
 *
 * Automerge schema types for Quarto collaborative documents.
 * These types define the structure of documents stored in Automerge
 * for real-time collaborative editing of Quarto projects.
 */

// ============================================================================
// Document Schema Types
// ============================================================================

/** Current schema version for IndexDocument. */
export const CURRENT_SCHEMA_VERSION = 1;

/**
 * Actor identity stored in the index document.
 * Older documents may store just a string (screen name); new entries
 * include both name and cursor color.
 */
export interface ActorIdentity {
  name: string;
  color: string; // hex color from the palette, e.g. "#E91E63"
}

/**
 * Root document that maps file paths to Automerge document IDs.
 * This is the entry point for a Quarto project in Automerge.
 *
 * `version` and `identities` are optional because V0 documents
 * (created before schema versioning) will not have them.
 */
export interface IndexDocument {
  files: Record<string, string>; // path -> docId mapping
  version?: number; // schema version (1 = current)
  identities?: Record<string, ActorIdentity>; // actorId -> identity
}

/**
 * Migrate an IndexDocument to the current schema version.
 * Must be called inside an Automerge `change()` callback.
 *
 * @returns true if the document was modified (migration applied)
 */
export function migrateIndexDocument(doc: IndexDocument): boolean {
  if (doc.version !== undefined) return false;
  doc.version = CURRENT_SCHEMA_VERSION;
  if (!doc.identities) {
    doc.identities = {};
  }
  return true;
}

/**
 * Set or update an identity mapping in the IndexDocument.
 * Must be called inside an Automerge `change()` callback.
 *
 * @returns true if the identity was added or changed
 */
export function setIdentity(doc: IndexDocument, actorId: string, screenName: string, color: string): boolean {
  if (!doc.identities) {
    doc.identities = {};
  }
  const existing = doc.identities[actorId];
  if (existing?.name === screenName && existing?.color === color) {
    return false;
  }
  doc.identities[actorId] = { name: screenName, color };
  return true;
}

/**
 * Text document content (e.g., .qmd, .yml files).
 * Identified by presence of 'text' field.
 */
export interface TextDocumentContent {
  text: string; // Automerge Text type serializes to string
}

/**
 * Binary document content (e.g., images, PDFs).
 * Identified by presence of 'content' field.
 */
export interface BinaryDocumentContent {
  content: Uint8Array; // raw binary data
  mimeType: string; // e.g., "image/png"
  hash: string; // SHA-256 hash (hex-encoded) for deduplication
}

/**
 * Union type for all document content types.
 */
export type FileDocumentContent = TextDocumentContent | BinaryDocumentContent;

/**
 * Document type enumeration.
 */
export type DocumentType = 'text' | 'binary' | 'invalid';

// ============================================================================
// File Entry Types
// ============================================================================

/**
 * File entry from IndexDocument.
 * Maps file paths to Automerge document IDs.
 */
export interface FileEntry {
  path: string;
  docId: string;
}

// ============================================================================
// Type Guards
// ============================================================================

/**
 * Type guard for text documents.
 */
export function isTextDocument(doc: unknown): doc is TextDocumentContent {
  return (
    typeof doc === 'object' &&
    doc !== null &&
    'text' in doc &&
    !('content' in doc)
  );
}

/**
 * Type guard for binary documents.
 */
export function isBinaryDocument(doc: unknown): doc is BinaryDocumentContent {
  return (
    typeof doc === 'object' &&
    doc !== null &&
    'content' in doc &&
    !('text' in doc)
  );
}

/**
 * Detect document type from content.
 */
export function getDocumentType(doc: unknown): DocumentType {
  if (isTextDocument(doc)) return 'text';
  if (isBinaryDocument(doc)) return 'binary';
  return 'invalid';
}

// ============================================================================
// File Type Detection (from extension)
// ============================================================================

/**
 * Known binary file extensions.
 */
const BINARY_EXTENSIONS = new Set([
  // Images
  'png',
  'jpg',
  'jpeg',
  'gif',
  'webp',
  'svg',
  'ico',
  'bmp',
  'tiff',
  'tif',
  // Documents
  'pdf',
  // Fonts
  'woff',
  'woff2',
  'ttf',
  'otf',
  'eot',
  // Audio/Video
  'mp3',
  'mp4',
  'webm',
  'ogg',
  'wav',
]);

/**
 * Known text file extensions.
 */
const TEXT_EXTENSIONS = new Set([
  'qmd',
  'md',
  'yml',
  'yaml',
  'json',
  'txt',
  'html',
  'css',
  'js',
  'ts',
  'tsx',
  'jsx',
]);

/**
 * Get file extension from path (lowercase, without dot).
 */
export function getFileExtension(path: string): string {
  const lastDot = path.lastIndexOf('.');
  if (lastDot === -1 || lastDot === path.length - 1) return '';
  return path.slice(lastDot + 1).toLowerCase();
}

/**
 * Check if a file path indicates a binary file (by extension).
 * This is a heuristic for UI display; the actual document content is the source of truth.
 */
export function isBinaryExtension(path: string): boolean {
  const ext = getFileExtension(path);
  return BINARY_EXTENSIONS.has(ext);
}

/**
 * Check if a file path indicates a text file (by extension).
 */
export function isTextExtension(path: string): boolean {
  const ext = getFileExtension(path);
  return TEXT_EXTENSIONS.has(ext);
}

/**
 * Infer MIME type from file extension.
 */
export function inferMimeType(path: string): string {
  const ext = getFileExtension(path);
  const mimeTypes: Record<string, string> = {
    // Images
    png: 'image/png',
    jpg: 'image/jpeg',
    jpeg: 'image/jpeg',
    gif: 'image/gif',
    webp: 'image/webp',
    svg: 'image/svg+xml',
    ico: 'image/x-icon',
    bmp: 'image/bmp',
    tiff: 'image/tiff',
    tif: 'image/tiff',
    // Documents
    pdf: 'application/pdf',
    // Fonts
    woff: 'font/woff',
    woff2: 'font/woff2',
    ttf: 'font/ttf',
    otf: 'font/otf',
    // Audio/Video
    mp3: 'audio/mpeg',
    mp4: 'video/mp4',
    webm: 'video/webm',
    ogg: 'audio/ogg',
    wav: 'audio/wav',
  };
  return mimeTypes[ext] || 'application/octet-stream';
}
