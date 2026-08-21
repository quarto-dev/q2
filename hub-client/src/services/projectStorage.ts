/**
 * IndexedDB-based storage for project entries.
 *
 * This module provides CRUD operations for project entries and integrates
 * with the schema versioning/migration system.
 */
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';
import { projectSetKey } from '@quarto/quarto-automerge-schema';
import type { ExportData, UserSettings } from './storage/types';
import { getCollectionPointers } from './projectSetStorage';
import {
  STORES,
  CURRENT_SCHEMA_VERSION,
  getDb,
  resetDbPromise,
  getSchemaVersion,
} from './storage';

/**
 * Generate a unique ID for a new project entry.
 */
function generateId(): string {
  return crypto.randomUUID();
}

/**
 * List all projects, ordered by last accessed (most recent first).
 */
export async function listProjects(): Promise<ProjectEntry[]> {
  const db = await getDb();
  const tx = db.transaction(STORES.PROJECTS, 'readonly');
  const store = tx.objectStore(STORES.PROJECTS);
  const index = store.index('lastAccessed');
  const entries = await index.getAll();
  return entries.reverse(); // Most recent first
}

/**
 * Get a single project by ID.
 */
export async function getProject(id: string): Promise<ProjectEntry | undefined> {
  const db = await getDb();
  return db.get(STORES.PROJECTS, id);
}

/**
 * Get a project by its index document ID.
 */
export async function getProjectByIndexDocId(indexDocId: string): Promise<ProjectEntry | undefined> {
  const db = await getDb();
  const tx = db.transaction(STORES.PROJECTS, 'readonly');
  const store = tx.objectStore(STORES.PROJECTS);
  const index = store.index('indexDocId');
  return index.get(indexDocId);
}

/**
 * Add a new project entry.
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
  await db.put(STORES.PROJECTS, entry);
  return entry;
}

/**
 * Update a project entry.
 */
export async function updateProject(entry: ProjectEntry): Promise<void> {
  const db = await getDb();
  await db.put(STORES.PROJECTS, entry);
}

/**
 * Update the last accessed timestamp for a project.
 */
export async function touchProject(id: string): Promise<void> {
  const db = await getDb();
  const entry = await db.get(STORES.PROJECTS, id);
  if (entry) {
    entry.lastAccessed = new Date().toISOString();
    await db.put(STORES.PROJECTS, entry);
  }
}

/**
 * Delete a project entry.
 */
export async function deleteProject(id: string): Promise<void> {
  const db = await getDb();
  await db.delete(STORES.PROJECTS, id);
}

/**
 * Delete every project entry matching an index document ID.
 *
 * Historical rows exist both with and without the `automerge:` prefix
 * (see projectSetReconciler), so both variants are matched. The reconciler
 * uses this to purge rows that lost to a deletion tombstone, so a stale
 * local copy can never resurrect a deleted project.
 *
 * @returns The number of rows deleted.
 */
export async function deleteProjectByIndexDocId(indexDocId: string): Promise<number> {
  const db = await getDb();
  const key = projectSetKey(indexDocId);
  const tx = db.transaction(STORES.PROJECTS, 'readwrite');
  const store = tx.objectStore(STORES.PROJECTS);
  const index = store.index('indexDocId');
  let deleted = 0;
  for (const variant of [key, `automerge:${key}`]) {
    const primaryKey = await index.getKey(variant);
    if (primaryKey !== undefined) {
      await store.delete(primaryKey);
      deleted++;
    }
  }
  await tx.done;
  return deleted;
}

/**
 * Export all data as JSON with schema version.
 *
 * The exported data includes:
 * - Schema version for import compatibility
 * - All project entries
 * - User settings (if present)
 */
export async function exportData(): Promise<string> {
  const db = await getDb();
  const schemaVersion = await getSchemaVersion(db);
  const projects = await listProjects();

  // Get user settings if the store exists
  let userSettings: UserSettings | undefined;
  if (db.objectStoreNames.contains(STORES.USER_SETTINGS)) {
    userSettings = await db.get(STORES.USER_SETTINGS, 'identity');
  }

  // Collection pointers: collections are synced ProjectSetDocuments, so the
  // docId + syncServer pair is everything a restoring browser needs to
  // re-subscribe (names/membership arrive with sync).
  const pointers = await getCollectionPointers();

  const exportData: ExportData = {
    schemaVersion,
    exportedAt: new Date().toISOString(),
    projects,
    userSettings,
    collections: pointers.map((p) => ({
      projectSetDocId: p.projectSetDocId,
      syncServer: p.syncServer,
    })),
  };

  return JSON.stringify(exportData, null, 2);
}

/**
 * Import data from JSON export.
 *
 * Handles both old format (array of projects) and new format (ExportData with version).
 * Returns count of successfully imported projects.
 */
export async function importData(json: string): Promise<number> {
  const parsed = JSON.parse(json);
  const db = await getDb();
  let count = 0;

  // Detect format: new format has schemaVersion, old format is an array
  let projects: ProjectEntry[];
  let userSettings: UserSettings | undefined;

  if (Array.isArray(parsed)) {
    // Old format: plain array of projects
    projects = parsed;
  } else if (parsed.schemaVersion !== undefined) {
    // New format: ExportData object
    const exportData = parsed as ExportData;
    projects = exportData.projects;
    userSettings = exportData.userSettings;

    // Note: We don't currently transform data based on schemaVersion differences,
    // but having the version in the export enables this in the future.
    if (exportData.schemaVersion > CURRENT_SCHEMA_VERSION) {
      console.warn(
        `Import data is from a newer schema version (${exportData.schemaVersion} > ${CURRENT_SCHEMA_VERSION}). ` +
        'Some data may not be compatible.'
      );
    }
  } else {
    throw new Error('Invalid import format: expected array of projects or ExportData object');
  }

  // Import projects
  for (const project of projects) {
    // Check if project with same indexDocId already exists
    const existing = await getProjectByIndexDocId(project.indexDocId);
    if (!existing) {
      // Generate new local ID
      const entry: ProjectEntry = {
        ...project,
        id: generateId(),
      };
      await db.put(STORES.PROJECTS, entry);
      count++;
    }
  }

  // Import user settings if provided and store exists
  if (userSettings && db.objectStoreNames.contains(STORES.USER_SETTINGS)) {
    const existingSettings = await db.get(STORES.USER_SETTINGS, 'identity');
    if (!existingSettings) {
      // Only import if no existing settings
      await db.put(STORES.USER_SETTINGS, {
        ...userSettings,
        updatedAt: new Date().toISOString(),
      });
    }
  }

  return count;
}

/**
 * @deprecated Use exportData() instead. Kept for backwards compatibility.
 */
export async function exportProjects(): Promise<string> {
  return exportData();
}

/**
 * @deprecated Use importData() instead. Kept for backwards compatibility.
 */
export async function importProjects(json: string): Promise<number> {
  return importData(json);
}

/**
 * Get the current schema version of the database.
 */
export async function getDatabaseSchemaVersion(): Promise<number> {
  const db = await getDb();
  return getSchemaVersion(db);
}

/**
 * Close the database connection.
 * Call this when you need to force a reconnection (e.g., after schema changes).
 */
export function closeDatabase(): void {
  getDb().then(db => db.close()).catch(() => {});
  resetDbPromise();
}
