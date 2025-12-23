/**
 * IndexedDB-based storage for project entries
 */
import { openDB } from 'idb';
import type { IDBPDatabase } from 'idb';
import type { ProjectEntry } from '../types/project';

const DB_NAME = 'quarto-hub';
const DB_VERSION = 1;
const STORE_NAME = 'projects';

let dbPromise: Promise<IDBPDatabase> | null = null;

async function getDb(): Promise<IDBPDatabase> {
  if (!dbPromise) {
    dbPromise = openDB(DB_NAME, DB_VERSION, {
      upgrade(db) {
        if (!db.objectStoreNames.contains(STORE_NAME)) {
          const store = db.createObjectStore(STORE_NAME, { keyPath: 'id' });
          store.createIndex('indexDocId', 'indexDocId', { unique: true });
          store.createIndex('lastAccessed', 'lastAccessed');
        }
      },
    });
  }
  return dbPromise;
}

/**
 * Generate a unique ID for a new project entry
 */
function generateId(): string {
  return crypto.randomUUID();
}

/**
 * List all projects, ordered by last accessed (most recent first)
 */
export async function listProjects(): Promise<ProjectEntry[]> {
  const db = await getDb();
  const tx = db.transaction(STORE_NAME, 'readonly');
  const store = tx.objectStore(STORE_NAME);
  const index = store.index('lastAccessed');
  const entries = await index.getAll();
  return entries.reverse(); // Most recent first
}

/**
 * Get a single project by ID
 */
export async function getProject(id: string): Promise<ProjectEntry | undefined> {
  const db = await getDb();
  return db.get(STORE_NAME, id);
}

/**
 * Get a project by its index document ID
 */
export async function getProjectByIndexDocId(indexDocId: string): Promise<ProjectEntry | undefined> {
  const db = await getDb();
  const tx = db.transaction(STORE_NAME, 'readonly');
  const store = tx.objectStore(STORE_NAME);
  const index = store.index('indexDocId');
  return index.get(indexDocId);
}

/**
 * Add a new project entry
 */
export async function addProject(
  indexDocId: string,
  syncServer: string,
  description?: string
): Promise<ProjectEntry> {
  const now = new Date().toISOString();
  const entry: ProjectEntry = {
    id: generateId(),
    indexDocId,
    syncServer,
    description: description || `Project ${now}`,
    createdAt: now,
    lastAccessed: now,
  };

  const db = await getDb();
  await db.put(STORE_NAME, entry);
  return entry;
}

/**
 * Update a project entry
 */
export async function updateProject(entry: ProjectEntry): Promise<void> {
  const db = await getDb();
  await db.put(STORE_NAME, entry);
}

/**
 * Update the last accessed timestamp for a project
 */
export async function touchProject(id: string): Promise<void> {
  const db = await getDb();
  const entry = await db.get(STORE_NAME, id);
  if (entry) {
    entry.lastAccessed = new Date().toISOString();
    await db.put(STORE_NAME, entry);
  }
}

/**
 * Delete a project entry
 */
export async function deleteProject(id: string): Promise<void> {
  const db = await getDb();
  await db.delete(STORE_NAME, id);
}

/**
 * Export all projects as JSON (for migration)
 */
export async function exportProjects(): Promise<string> {
  const projects = await listProjects();
  return JSON.stringify(projects, null, 2);
}

/**
 * Import projects from JSON (for migration)
 * Returns count of successfully imported projects
 */
export async function importProjects(json: string): Promise<number> {
  const projects = JSON.parse(json) as ProjectEntry[];
  const db = await getDb();
  let count = 0;

  for (const project of projects) {
    // Check if project with same indexDocId already exists
    const existing = await getProjectByIndexDocId(project.indexDocId);
    if (!existing) {
      // Generate new local ID
      const entry: ProjectEntry = {
        ...project,
        id: generateId(),
      };
      await db.put(STORE_NAME, entry);
      count++;
    }
  }

  return count;
}
