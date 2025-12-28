/**
 * Migration runner for IndexedDB schema evolution.
 *
 * Handles running data transformation migrations after the database is open.
 * Structural migrations (store/index creation) are handled separately in the
 * IndexedDB upgrade callback.
 */

import type { HubDatabase, SchemaMeta, MigrationRecord } from './types';
import { STORES } from './types';
import {
  getMigrationsFrom,
  BASELINE_SCHEMA_VERSION,
  CURRENT_SCHEMA_VERSION,
} from './migrations';

/**
 * Error thrown when a migration fails.
 */
export class MigrationFailedError extends Error {
  readonly version: number;
  readonly cause: unknown;

  constructor(version: number, cause: unknown) {
    const message = cause instanceof Error ? cause.message : String(cause);
    super(`Migration to version ${version} failed: ${message}`);
    this.name = 'MigrationFailedError';
    this.version = version;
    this.cause = cause;
  }
}

/**
 * Get the current schema metadata from the database.
 * Returns null if the _meta store doesn't exist or has no schema record.
 */
async function getSchemaMeta(db: HubDatabase): Promise<SchemaMeta | null> {
  try {
    // Check if _meta store exists
    if (!db.objectStoreNames.contains(STORES.META)) {
      return null;
    }
    return await db.get(STORES.META, 'schema') ?? null;
  } catch {
    // Store might not exist yet (pre-migration database)
    return null;
  }
}

/**
 * Initialize or update the schema metadata.
 */
async function setSchemaMeta(db: HubDatabase, meta: SchemaMeta): Promise<void> {
  await db.put(STORES.META, meta);
}

/**
 * Record a successful migration in the schema metadata.
 */
async function recordMigrationSuccess(
  db: HubDatabase,
  version: number,
  durationMs: number
): Promise<void> {
  const meta = await getSchemaMeta(db);
  if (!meta) {
    // This shouldn't happen if migrations run in order, but handle it
    throw new Error('Schema metadata not found when recording migration success');
  }

  const record: MigrationRecord = {
    version,
    appliedAt: new Date().toISOString(),
    durationMs,
  };

  const updatedMeta: SchemaMeta = {
    ...meta,
    version,
    migrationsApplied: [...meta.migrationsApplied, record],
    // Clear any previous error since we succeeded
    lastMigrationError: undefined,
  };

  await setSchemaMeta(db, updatedMeta);
}

/**
 * Record a failed migration in the schema metadata.
 */
async function recordMigrationError(
  db: HubDatabase,
  version: number,
  error: unknown
): Promise<void> {
  const meta = await getSchemaMeta(db);
  if (!meta) {
    // Can't record error if meta doesn't exist
    console.error('Cannot record migration error: schema metadata not found');
    return;
  }

  const updatedMeta: SchemaMeta = {
    ...meta,
    lastMigrationError: {
      version,
      error: error instanceof Error ? error.message : String(error),
      occurredAt: new Date().toISOString(),
    },
  };

  await setSchemaMeta(db, updatedMeta);
}

/**
 * Initialize schema metadata for a database that existed before the migration system.
 * This is called after the IndexedDB upgrade creates the _meta store.
 */
export async function initializeSchemaMeta(
  db: HubDatabase,
  initialVersion: number = BASELINE_SCHEMA_VERSION
): Promise<SchemaMeta> {
  const existingMeta = await getSchemaMeta(db);
  if (existingMeta) {
    return existingMeta;
  }

  const meta: SchemaMeta = {
    key: 'schema',
    version: initialVersion,
    migrationsApplied: [],
  };

  await setSchemaMeta(db, meta);
  return meta;
}

/**
 * Run all pending data transformation migrations.
 *
 * This should be called after the database is open and any structural
 * migrations have been applied via the IndexedDB upgrade callback.
 *
 * @param db - The open database instance
 * @throws MigrationFailedError if any migration fails
 */
export async function runMigrations(db: HubDatabase): Promise<void> {
  // Ensure _meta store exists and has initial metadata
  if (!db.objectStoreNames.contains(STORES.META)) {
    // This means the IndexedDB upgrade didn't run (shouldn't happen)
    console.warn('_meta store not found, skipping migrations');
    return;
  }

  // Initialize or get current schema metadata
  const meta = await initializeSchemaMeta(db);

  // Check if we're already at the current version
  if (meta.version >= CURRENT_SCHEMA_VERSION) {
    return;
  }

  // Get pending migrations
  const pendingMigrations = getMigrationsFrom(meta.version);

  if (pendingMigrations.length === 0) {
    return;
  }

  console.log(
    `Running ${pendingMigrations.length} migration(s) from v${meta.version} to v${CURRENT_SCHEMA_VERSION}`
  );

  // Run each migration in order
  for (const migration of pendingMigrations) {
    console.log(`Running migration v${migration.version}: ${migration.description}`);
    const startTime = performance.now();

    try {
      // Only run the transform part here; structural changes were done in upgrade
      if (migration.transform) {
        await migration.transform(db);
      }

      const durationMs = Math.round(performance.now() - startTime);
      await recordMigrationSuccess(db, migration.version, durationMs);

      console.log(`Migration v${migration.version} completed in ${durationMs}ms`);
    } catch (error) {
      console.error(`Migration v${migration.version} failed:`, error);
      await recordMigrationError(db, migration.version, error);
      throw new MigrationFailedError(migration.version, error);
    }
  }

  console.log(`All migrations completed. Schema is now at v${CURRENT_SCHEMA_VERSION}`);
}

/**
 * Get the current schema version from the database.
 * Returns BASELINE_SCHEMA_VERSION if no metadata exists.
 */
export async function getSchemaVersion(db: HubDatabase): Promise<number> {
  const meta = await getSchemaMeta(db);
  return meta?.version ?? BASELINE_SCHEMA_VERSION;
}

/**
 * Check if there are pending migrations that need to run.
 */
export async function hasPendingMigrations(db: HubDatabase): Promise<boolean> {
  const currentVersion = await getSchemaVersion(db);
  return currentVersion < CURRENT_SCHEMA_VERSION;
}

/**
 * Get the last migration error, if any.
 */
export async function getLastMigrationError(
  db: HubDatabase
): Promise<SchemaMeta['lastMigrationError']> {
  const meta = await getSchemaMeta(db);
  return meta?.lastMigrationError;
}
