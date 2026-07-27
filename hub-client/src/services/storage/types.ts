/**
 * Type definitions for IndexedDB schema versioning and migration system.
 */

import type { IDBPDatabase, IDBPTransaction } from 'idb';

/**
 * Database name and store names as constants for consistency.
 */
export const DB_NAME = 'quarto-hub';

export const STORES = {
  META: '_meta',
  PROJECTS: 'projects',
  USER_SETTINGS: 'userSettings',
  PROJECT_SET: 'projectSet',
} as const;

/**
 * Schema metadata stored in the _meta store.
 * Tracks the current schema version and migration history.
 */
export interface SchemaMeta {
  key: 'schema';
  version: number;
  migrationsApplied: MigrationRecord[];
  lastMigrationError?: MigrationError;
}

/**
 * Record of a successfully applied migration.
 */
export interface MigrationRecord {
  version: number;
  appliedAt: string;
  durationMs: number;
}

/**
 * Record of a failed migration attempt.
 */
export interface MigrationError {
  version: number;
  error: string;
  occurredAt: string;
}

/**
 * Pointer to the user's Automerge-backed project set document.
 * Stored as a singleton in the projectSet store (key: 'projectSet').
 *
 * This is the only project-related data that remains in IndexedDB after
 * migration — the actual project list lives in the Automerge document.
 */
export interface ProjectSetPointer {
  key: 'projectSet';
  /** Automerge document ID for the ProjectSetDocument. */
  projectSetDocId: string;
  /** Sync server URL where the project set document is hosted. */
  syncServer: string;
}

/**
 * One collection the user is subscribed to: a pointer to a
 * ProjectSetDocument acting as a collection.
 */
export interface CollectionPointerEntry {
  /** Automerge document ID for the collection's ProjectSetDocument. */
  projectSetDocId: string;
  /** Sync server URL where the document is hosted. */
  syncServer: string;
}

/**
 * The root of the user's QuartoHub state: the set of collections this
 * browser is subscribed to. Stored as a singleton in the projectSet store
 * (key: 'collections'), alongside the legacy singleton pointer.
 *
 * The first entry is the personal root collection (the migrated legacy
 * project set); later entries are additional/shared collections in
 * subscription order.
 */
export interface CollectionsPointer {
  key: 'collections';
  collections: CollectionPointerEntry[];
}

/**
 * User identity settings for presence features.
 * Stored as a singleton in the userSettings store.
 */
export interface UserSettings {
  key: 'identity';
  userId: string;
  userName: string;
  userColor: string;
  createdAt: string;
  updatedAt: string;
}

/**
 * Migration definition.
 *
 * Migrations can have two components:
 * - structural: Changes to object stores and indexes (runs in IndexedDB upgrade)
 * - transform: Data transformations (runs after DB is open, can be async)
 */
export interface Migration {
  /** Target schema version after this migration completes */
  version: number;

  /** Human-readable description of what this migration does */
  description: string;

  /**
   * Structural changes to the database schema.
   * Runs synchronously during IndexedDB's upgrade callback.
   * Use for creating/deleting object stores and indexes.
   *
   * Note: The db parameter is IDBPDatabase from the idb library's upgrade callback.
   * It provides the same createObjectStore/deleteObjectStore methods as IDBDatabase.
   */
  structural?: (db: IDBPDatabase, transaction: IDBPTransaction<unknown, string[], 'versionchange'>) => void;

  /**
   * Data transformation logic.
   * Runs asynchronously after the database is open.
   * Use for modifying existing data or initializing new fields.
   */
  transform?: (db: IDBPDatabase) => Promise<void>;
}

/**
 * Export data format with schema versioning.
 * Used for backup/restore functionality.
 */
export interface ExportData {
  schemaVersion: number;
  exportedAt: string;
  projects: ProjectEntryV2[];
  userSettings?: UserSettings;
}

/**
 * Project entry stored in IndexedDB (current version).
 * This mirrors the ProjectEntry type but is defined here for migration purposes.
 */
export interface ProjectEntryV2 {
  id: string;
  indexDocId: string;
  syncServer: string;
  description: string;
  createdAt: string;
  lastAccessed: string;
}

/**
 * Type alias for the database instance used throughout the migration system.
 */
export type HubDatabase = IDBPDatabase;

/**
 * Type for the upgrade transaction provided by IndexedDB.
 */
export type UpgradeTransaction = IDBPTransaction<unknown, string[], 'versionchange'>;
