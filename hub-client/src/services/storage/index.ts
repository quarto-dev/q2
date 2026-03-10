/**
 * Storage module exports.
 *
 * This module provides IndexedDB storage with schema versioning and migration support.
 */

// Types
export type {
  SchemaMeta,
  MigrationRecord,
  MigrationError,
  UserSettings,
  Migration,
  ExportData,
  ProjectEntryV2,
  HubDatabase,
} from './types';

export { DB_NAME, STORES } from './types';

// Migrations
export {
  CURRENT_DB_VERSION,
  CURRENT_SCHEMA_VERSION,
  BASELINE_SCHEMA_VERSION,
  migrations,
  getMigrationsFrom,
  getStructuralMigrations,
  getMigration,
} from './migrations';

// Migration runner
export {
  runMigrations,
  getSchemaVersion,
  hasPendingMigrations,
  getLastMigrationError,
  initializeSchemaMeta,
  MigrationFailedError,
} from './migrationRunner';

// Utilities
export {
  generateColorFromId,
  generateAnonymousName,
  isValidHexColor,
  isValidUserName,
} from './utils';
