/**
 * Migration registry for IndexedDB schema evolution.
 *
 * ============================================================================
 * ADDING A NEW MIGRATION?
 * See: claude-notes/instructions/hub-client-storage.md
 *
 * Quick checklist:
 * 1. Increment CURRENT_SCHEMA_VERSION (and CURRENT_DB_VERSION if structural)
 * 2. Add migration object to the `migrations` array below
 * 3. Update types in ./types.ts if adding new stores or fields
 * 4. Test both fresh install and upgrade paths
 * ============================================================================
 *
 * This file defines all migrations and provides helpers for querying them.
 *
 * Versioning strategy:
 * - CURRENT_DB_VERSION: IndexedDB version number, triggers structural changes
 * - CURRENT_SCHEMA_VERSION: Application schema version, tracks data transformations
 *
 * These can diverge: a data-only migration bumps schema version but not DB version.
 */

import type { Migration } from './types';
import { STORES } from './types';
import { generateColorFromId, generateAnonymousName } from './utils';

/**
 * Current IndexedDB version.
 * Increment this when adding/removing object stores or indexes.
 */
export const CURRENT_DB_VERSION = 4;

/**
 * Current application schema version.
 * This is the version number after all migrations have been applied.
 */
export const CURRENT_SCHEMA_VERSION = 5;

/**
 * Baseline schema version for databases that existed before the migration system.
 * If a database has no _meta store, we assume it's at this version.
 */
export const BASELINE_SCHEMA_VERSION = 1;

/**
 * All migrations, in order.
 *
 * Each migration upgrades from version N-1 to version N.
 * Migrations must be idempotent where possible.
 *
 * Migration 1→2: Add migration infrastructure and user identity
 * - Structural: Create _meta store for tracking schema version
 * - Structural: Create userSettings store for user identity
 * - Transform: Initialize default user identity
 */
export const migrations: Migration[] = [
  {
    version: 2,
    description: 'Add schema metadata tracking and user identity storage',
    structural: (db) => {
      // Create _meta store for schema versioning
      // This store tracks the current schema version and migration history
      if (!db.objectStoreNames.contains(STORES.META)) {
        db.createObjectStore(STORES.META, { keyPath: 'key' });
      }

      // Create userSettings store for user identity (presence features)
      if (!db.objectStoreNames.contains(STORES.USER_SETTINGS)) {
        db.createObjectStore(STORES.USER_SETTINGS, { keyPath: 'key' });
      }
    },
    transform: async (db) => {
      // Initialize default user identity if not present
      const existingSettings = await db.get(STORES.USER_SETTINGS, 'identity');
      if (!existingSettings) {
        const userId = crypto.randomUUID();
        const now = new Date().toISOString();
        await db.put(STORES.USER_SETTINGS, {
          key: 'identity',
          userId,
          userName: generateAnonymousName(),
          userColor: generateColorFromId(userId),
          createdAt: now,
          updatedAt: now,
        });
      }
    },
  },
  // Migration 2→3: Previously created sassCache store. SASS caching has moved
  // to the quarto-cache IndexedDB (via wasm-js-bridge/cache.js). This migration
  // is kept as a no-op so existing v3 databases don't trigger version mismatches.
  {
    version: 3,
    description: 'No-op (sassCache store is now inert — caching moved to quarto-cache DB)',
  },
  // Migration 3→4: Add projectSet store for Automerge-backed project set pointer.
  // The actual project list moves to an Automerge document; IndexedDB stores only
  // a pointer to that document. The old 'projects' store is kept as a safety net
  // for migration and will be removed in a future version.
  {
    version: 4,
    description: 'Add projectSet store for synced project list pointer',
    structural: (db) => {
      if (!db.objectStoreNames.contains(STORES.PROJECT_SET)) {
        db.createObjectStore(STORES.PROJECT_SET, { keyPath: 'key' });
      }
    },
  },
  // Migration 4→5: Collections. The root pointer goes from a single project
  // set to an array of collection pointers (each collection is its own
  // ProjectSetDocument). Transform-only: the array lives in the existing
  // projectSet store under the 'collections' key, and the legacy singleton
  // pointer is kept untouched as a safety net.
  {
    version: 5,
    description: 'Convert singleton project set pointer to collections array',
    transform: async (db) => {
      await migratePointerToCollections(db);
    },
  },
];

/**
 * Convert the legacy singleton project set pointer into a one-element
 * collections array. Idempotent: a no-op when the collections record
 * already exists or there is nothing to migrate. The legacy pointer is
 * never deleted here.
 *
 * Exported for direct unit testing and for lazy self-healing reads
 * (see projectSetStorage.getCollectionPointers).
 */
export async function migratePointerToCollections(
  db: import('idb').IDBPDatabase,
): Promise<void> {
  if (!db.objectStoreNames.contains(STORES.PROJECT_SET)) return;
  const existing = await db.get(STORES.PROJECT_SET, 'collections');
  if (existing) return;
  const legacy = await db.get(STORES.PROJECT_SET, 'projectSet');
  if (!legacy) return;
  await db.put(STORES.PROJECT_SET, {
    key: 'collections',
    collections: [
      { projectSetDocId: legacy.projectSetDocId, syncServer: legacy.syncServer },
    ],
  });
}

/**
 * Get migrations that need to be applied to upgrade from a given version.
 * Returns migrations in order, from lowest to highest version.
 */
export function getMigrationsFrom(fromVersion: number): Migration[] {
  return migrations
    .filter((m) => m.version > fromVersion)
    .sort((a, b) => a.version - b.version);
}

/**
 * Get only the structural parts of migrations for the IndexedDB upgrade callback.
 * These run synchronously during database open.
 */
export function getStructuralMigrations(
  fromDbVersion: number
): Migration[] {
  // For structural migrations, we use the IndexedDB version (1-indexed)
  // to determine which migrations to run
  return migrations
    .filter((m) => m.version > fromDbVersion && m.structural !== undefined)
    .sort((a, b) => a.version - b.version);
}

/**
 * Get the migration for a specific version.
 */
export function getMigration(version: number): Migration | undefined {
  return migrations.find((m) => m.version === version);
}
