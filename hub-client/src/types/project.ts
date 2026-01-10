/**
 * Project entry stored in IndexedDB
 * Contains the connection information for an automerge project
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
 * File entry from IndexDocument
 * Maps file paths to automerge document IDs
 */
export interface FileEntry {
  path: string;
  docId: string;
}

/**
 * State for the currently selected project
 */
export interface ProjectState {
  entry: ProjectEntry;
  files: FileEntry[];
  currentFile: FileEntry | null;
  connected: boolean;
}

// ============================================================================
// Document Content Types
// ============================================================================

/**
 * Text document content (e.g., .qmd, .yml files)
 * Identified by presence of 'text' field
 */
export interface TextDocumentContent {
  text: string; // automerge Text type serializes to string
}

/**
 * Binary document content (e.g., images, PDFs)
 * Identified by presence of 'content' field
 */
export interface BinaryDocumentContent {
  content: Uint8Array; // raw binary data
  mimeType: string; // e.g., "image/png"
  hash: string; // SHA-256 hash (hex-encoded)
}

/**
 * Union type for all document content types
 */
export type FileDocumentContent = TextDocumentContent | BinaryDocumentContent;

/**
 * Document type enumeration
 */
export type DocumentType = 'text' | 'binary' | 'invalid';

/**
 * Type guard for text documents
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
 * Type guard for binary documents
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
 * Detect document type from content
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
 * Known binary file extensions
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
 * Known text file extensions
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
 * Get file extension from path (lowercase, without dot)
 */
export function getFileExtension(path: string): string {
  const lastDot = path.lastIndexOf('.');
  if (lastDot === -1 || lastDot === path.length - 1) return '';
  return path.slice(lastDot + 1).toLowerCase();
}

/**
 * Check if a file path indicates a binary file (by extension)
 * This is a heuristic for UI display; the actual document content is the source of truth.
 */
export function isBinaryExtension(path: string): boolean {
  const ext = getFileExtension(path);
  return BINARY_EXTENSIONS.has(ext);
}

/**
 * Check if a file path indicates a text file (by extension)
 */
export function isTextExtension(path: string): boolean {
  const ext = getFileExtension(path);
  return TEXT_EXTENSIONS.has(ext);
}

/**
 * Infer MIME type from file extension
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
