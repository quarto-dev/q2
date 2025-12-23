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
