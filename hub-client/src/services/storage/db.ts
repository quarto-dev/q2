/**
 * Shared database initialization.
 *
 * Both projectStorage and userSettings import getDb from here,
 * ensuring the upgrade callback always runs when the database is first created.
 */

import { openDB } from 'idb';
import type { IDBPDatabase } from 'idb';
import { DB_NAME, STORES } from './types';
import { CURRENT_DB_VERSION, getStructuralMigrations } from './migrations';
import { runMigrations } from './migrationRunner';

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
 *
 * The database instance is cached — subsequent calls return the same promise.
 */
export async function getDb(): Promise<IDBPDatabase> {
  if (!dbPromise) {
    dbPromise = (async () => {
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
 * Reset the cached database promise.
 * Call this if the database connection is lost or needs to be reopened.
 */
export function resetDbPromise(): void {
  dbPromise = null;
}
