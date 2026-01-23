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
export const CURRENT_DB_VERSION = 3;

/**
 * Current application schema version.
 * This is the version number after all migrations have been applied.
 */
export const CURRENT_SCHEMA_VERSION = 3;

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
  // Migration 2→3: Add SASS compilation cache
  {
    version: 3,
    description: 'Add SASS compilation cache for faster subsequent renders',
    structural: (db) => {
      // Create sassCache store for caching compiled CSS
      // Uses LRU eviction based on lastUsed timestamp
      if (!db.objectStoreNames.contains(STORES.SASS_CACHE)) {
        const store = db.createObjectStore(STORES.SASS_CACHE, { keyPath: 'key' });
        // Index for LRU eviction (oldest entries first)
        store.createIndex('lastUsed', 'lastUsed');
        // Index for size-based queries during eviction
        store.createIndex('size', 'size');
      }
    },
    // No transform needed - cache starts empty
  },
];

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
