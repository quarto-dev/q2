/**
 * IndexedDB-based storage for project entries.
 *
 * This module provides CRUD operations for project entries and integrates
 * with the schema versioning/migration system.
 */
import { openDB } from 'idb';
import type { IDBPDatabase } from 'idb';
import type { ProjectEntry } from '../types/project';
import type { ExportData, UserSettings } from './storage/types';
import {
  DB_NAME,
  STORES,
  CURRENT_DB_VERSION,
  CURRENT_SCHEMA_VERSION,
  getStructuralMigrations,
  runMigrations,
  getSchemaVersion,
} from './storage';

/**
 * Cached database promise.
 * Reset to null if database needs to be reopened.
 */
let dbPromise: Promise<IDBPDatabase> | null = null;

/**
 * Get or open the database, running migrations as needed.
 *
 * This function:
 * 1. Opens the database with the current version
 * 2. Runs structural migrations (store/index creation) in the upgrade callback
 * 3. Runs data transformation migrations after the database is open
 */
async function getDb(): Promise<IDBPDatabase> {
  if (!dbPromise) {
    dbPromise = (async () => {
      // Open the database with structural migrations in the upgrade callback
      const db = await openDB(DB_NAME, CURRENT_DB_VERSION, {
        upgrade(db, oldVersion, _newVersion, transaction) {
          // Create projects store if this is a fresh database
          if (!db.objectStoreNames.contains(STORES.PROJECTS)) {
            const store = db.createObjectStore(STORES.PROJECTS, { keyPath: 'id' });
            store.createIndex('indexDocId', 'indexDocId', { unique: true });
            store.createIndex('lastAccessed', 'lastAccessed');
          }

          // Run structural migrations for version upgrades
          // oldVersion is 0 for new databases, so we start from 1
          const fromVersion = oldVersion || 1;
          const structuralMigrations = getStructuralMigrations(fromVersion);

          for (const migration of structuralMigrations) {
            if (migration.structural) {
              console.log(`Running structural migration v${migration.version}: ${migration.description}`);
              migration.structural(db, transaction);
            }
          }
        },
      });

      // Run data transformation migrations after the database is open
      await runMigrations(db);

      return db;
    })();
  }
  return dbPromise;
}

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

  const exportData: ExportData = {
    schemaVersion,
    exportedAt: new Date().toISOString(),
    projects,
    userSettings,
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
  if (dbPromise) {
    dbPromise.then(db => db.close()).catch(() => {});
    dbPromise = null;
  }
}
