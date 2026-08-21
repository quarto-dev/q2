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
export const CURRENT_SCHEMA_VERSION = 2;

/** Current schema version for ProjectSetDocument. */
export const CURRENT_PROJECT_SET_SCHEMA_VERSION = 1;

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
 * Reference to a recorded engine capture for a file in the index.
 *
 * Stored in the `captures` sidecar map (V2+) keyed by the same path used
 * in `files`. `captureDocId` points to a separate Automerge document
 * holding the serialized `EngineCapture`. The other fields are surface
 * state consumed by the q2 preview SPA.
 *
 * `state` is reserved as a string enum (rather than a boolean `executing`)
 * so future error/idle states can grow without another schema bump.
 */
export interface CaptureRef {
  captureDocId: string;
  staleness?: boolean;
  state?: 'idle' | 'running' | 'error';
  lastError?: string;
}

/**
 * Root document that maps file paths to Automerge document IDs.
 * This is the entry point for a Quarto project in Automerge.
 *
 * `version` and `identities` are optional because V0 documents
 * (created before schema versioning) will not have them.
 *
 * `captures` is a sidecar map (V2+) keyed by the same paths used in
 * `files`. Entries are absent until an engine capture is recorded for
 * the path; the q2 preview server writes them and the SPA reads them.
 */
export interface IndexDocument {
  files: Record<string, string>; // path -> docId mapping
  version?: number; // schema version (2 = current)
  identities?: Record<string, ActorIdentity>; // actorId -> identity
  captures?: Record<string, CaptureRef>; // path -> capture sidecar entry (V2+)
}

/**
 * Migrate an IndexDocument to the current schema version.
 * Must be called inside an Automerge `change()` callback.
 *
 * Steps (idempotent):
 *   V0 → V1: initialize `identities`, set `version = 1`.
 *   V1 → V2: bump `version = 2`. No structural change — `captures` is
 *            absent until a capture is recorded.
 *
 * Any preexisting `captures` sidecar (e.g. on a doc mis-tagged as V1)
 * is preserved through the bump.
 *
 * @returns true if the document was modified (migration applied)
 */
export function migrateIndexDocument(doc: IndexDocument): boolean {
  if (doc.version === CURRENT_SCHEMA_VERSION) return false;

  let changed = false;
  if (doc.version === undefined) {
    // V0 → V1
    if (!doc.identities) {
      doc.identities = {};
    }
    doc.version = 1;
    changed = true;
  }
  if (doc.version === 1) {
    // V1 → V2 (sidecar capture map; no structural change needed)
    doc.version = 2;
    changed = true;
  }
  return changed;
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

// ============================================================================
// Project Set Schema Types
// ============================================================================

/**
 * Entry in a ProjectSetDocument representing a single project.
 *
 * The key in the `projects` Record is the indexDocId without the 'automerge:'
 * prefix, which serves as a natural deduplication key — two browsers adding
 * the same project converge automatically via Automerge CRDT merge.
 */
/**
 * Cached at-a-glance summary of a project, denormalized onto the owner's
 * project-set entry.
 *
 * This is a per-user cache, not a source of truth: it reflects the project
 * as of the last time THIS user's client opened it (`asOf`), and is written
 * by the client while it has the project's documents in hand. It exists so
 * list surfaces (cards, peek popovers) can show file counts and contributors
 * without opening a sync connection per project.
 */
export interface ProjectSetEntrySummary {
  /** Number of files in the project's index at `asOf`. */
  fileCount: number;
  /** First few file paths (capped by the writer; typically 5). */
  topFiles: string[];
  /** Identities seen on the project at `asOf` (capped by the writer). */
  contributors: ActorIdentity[];
  /** ISO timestamp when this summary was captured. */
  asOf: string;
}

export interface ProjectSetEntry {
  /** Automerge document ID for the project's IndexDocument (with 'automerge:' prefix). */
  indexDocId: string;
  /** WebSocket URL for the sync server hosting this project. */
  syncServer: string;
  /** User-provided project name/description. */
  description: string;
  /** ISO timestamp when this project was added to the set. */
  addedAt: string;
  /** ISO timestamp of last access from any browser. Updated on project open. */
  lastAccessed: string;
  /** Cached peek data; absent until the project is first opened by this user. */
  summary?: ProjectSetEntrySummary;
}

/**
 * Root document for a user's project set, synced across browsers via Automerge.
 *
 * This replaces the per-browser IndexedDB project list. Each browser stores
 * only a pointer (the document ID of this ProjectSetDocument) in IndexedDB.
 *
 * The `projects` map is keyed by indexDocId without the 'automerge:' prefix.
 */
export interface ProjectSetDocument {
  /** Map of project entries, keyed by indexDocId (without 'automerge:' prefix). */
  projects: Record<string, ProjectSetEntry>;
  /** Schema version for forward compatibility. */
  version: number;
  /**
   * Display name when this set is used as a collection (a user-visible,
   * shareable grouping of projects). Absent on sets created before
   * collections existed; the UI supplies a default until the user names it.
   */
  name?: string;
  /**
   * Deletion tombstones: map key -> ISO timestamp of the deletion, written
   * by `removeProjectFromSet` and cleared by `addProjectToSet`. An entry
   * absent from `projects` is ambiguous without these — "never synced" and
   * "deleted" look identical — so reconcilers compare the tombstone against
   * a candidate's lastAccessed and let the latest one win. Optional:
   * documents created before tombstones existed simply lack the map until
   * the first deletion lazily initializes it.
   */
  tombstones?: Record<string, string>;
}

/**
 * Create an empty ProjectSetDocument.
 * Use this inside an Automerge `change()` callback to initialize a new document.
 */
export function initProjectSetDocument(doc: ProjectSetDocument): void {
  doc.projects = {};
  doc.version = CURRENT_PROJECT_SET_SCHEMA_VERSION;
}

/**
 * Strip the 'automerge:' prefix from a document ID to get the map key.
 */
export function projectSetKey(indexDocId: string): string {
  return indexDocId.replace(/^automerge:/, '');
}

/**
 * Add or update a project in a ProjectSetDocument.
 * Must be called inside an Automerge `change()` callback.
 *
 * @returns true if the entry was added or changed
 */
export function addProjectToSet(
  doc: ProjectSetDocument,
  entry: Omit<ProjectSetEntry, 'addedAt' | 'lastAccessed'>,
  now?: string,
): boolean {
  const key = projectSetKey(entry.indexDocId);
  const timestamp = now ?? new Date().toISOString();

  // A (re-)add wins over any earlier deletion tombstone — latest-wins.
  let changed = false;
  if (doc.tombstones && key in doc.tombstones) {
    delete doc.tombstones[key];
    changed = true;
  }

  const existing = doc.projects[key];

  if (existing) {
    // Update mutable fields if changed
    if (existing.description !== entry.description) {
      existing.description = entry.description;
      changed = true;
    }
    if (existing.syncServer !== entry.syncServer) {
      existing.syncServer = entry.syncServer;
      changed = true;
    }
    return changed;
  }

  doc.projects[key] = {
    indexDocId: entry.indexDocId,
    syncServer: entry.syncServer,
    description: entry.description,
    addedAt: timestamp,
    lastAccessed: timestamp,
  };
  return true;
}

/**
 * Remove a project from a ProjectSetDocument, leaving a deletion tombstone
 * (the key's deletion timestamp) so reconcilers can apply latest-wins
 * against stale local copies. Must be called inside an Automerge `change()`
 * callback.
 *
 * @returns true if the entry was removed
 */
export function removeProjectFromSet(
  doc: ProjectSetDocument,
  indexDocId: string,
  now?: string,
): boolean {
  const key = projectSetKey(indexDocId);
  if (!doc.projects[key]) return false;
  delete doc.projects[key];
  if (!doc.tombstones) doc.tombstones = {};
  doc.tombstones[key] = now ?? new Date().toISOString();
  return true;
}

/**
 * Read the deletion-tombstone map (key -> ISO deletion timestamp), or an
 * empty object when the document predates tombstones.
 */
export function getProjectSetTombstones(
  doc: ProjectSetDocument,
): Record<string, string> {
  return doc.tombstones ?? {};
}

/**
 * Update the lastAccessed timestamp for a project.
 * Must be called inside an Automerge `change()` callback.
 */
export function touchProjectInSet(
  doc: ProjectSetDocument,
  indexDocId: string,
  now?: string,
): boolean {
  const key = projectSetKey(indexDocId);
  const entry = doc.projects[key];
  if (!entry) return false;
  entry.lastAccessed = now ?? new Date().toISOString();
  return true;
}

/**
 * Set the collection display name on a ProjectSetDocument.
 * Must be called inside an Automerge `change()` callback.
 *
 * @returns true if the name changed
 */
export function setProjectSetName(
  doc: ProjectSetDocument,
  name: string,
): boolean {
  if (doc.name === name) return false;
  doc.name = name;
  return true;
}

/**
 * Update the cached peek summary for a project.
 *
 * The summary lives on a shared collection entry, so multiple collaborators
 * write it. The file-shape fields (fileCount, topFiles, asOf) are "last
 * writer's view" and replaced wholesale, but `contributors` are **unioned**
 * with whatever is already stored: a naive replace made each editor clobber
 * the others' identities, so the last person to edit appeared as the sole
 * author. Union (dedup by name, incoming color/order preferred) makes
 * authorship accumulate the way collaborators expect.
 *
 * Must be called inside an Automerge `change()` callback.
 *
 * @returns true if the entry existed and the summary was written
 */
export function updateProjectSummaryInSet(
  doc: ProjectSetDocument,
  indexDocId: string,
  summary: ProjectSetEntrySummary,
): boolean {
  const key = projectSetKey(indexDocId);
  const entry = doc.projects[key];
  if (!entry) return false;

  const merged = new Map<string, ActorIdentity>();
  for (const c of summary.contributors) merged.set(c.name, { name: c.name, color: c.color });
  for (const c of entry.summary?.contributors ?? []) {
    if (!merged.has(c.name)) merged.set(c.name, { name: c.name, color: c.color });
  }

  entry.summary = {
    fileCount: summary.fileCount,
    topFiles: summary.topFiles,
    asOf: summary.asOf,
    contributors: [...merged.values()],
  };
  return true;
}

// ============================================================================
// File Document Content Types
// ============================================================================

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
