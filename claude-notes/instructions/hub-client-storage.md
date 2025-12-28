# Hub Client Storage System

This document describes the IndexedDB storage system for `hub-client`, including schema versioning and migrations.

## Overview

The hub-client uses IndexedDB for persistent browser storage, managed through the `idb` library. The storage system includes:

- **Schema versioning**: Track what version of the data schema is stored
- **Migration system**: Safely evolve the schema over time without losing user data
- **User settings**: Store user identity for presence/collaboration features

## Key Files

| File | Purpose |
|------|---------|
| `src/services/storage/types.ts` | Type definitions for all storage-related interfaces |
| `src/services/storage/migrations.ts` | **Migration registry** - where new migrations are defined |
| `src/services/storage/migrationRunner.ts` | Executes migrations and tracks progress |
| `src/services/storage/utils.ts` | Utility functions (color generation, name generation) |
| `src/services/projectStorage.ts` | Project CRUD operations, database initialization |
| `src/services/userSettings.ts` | User identity management |

## How to Add a New Migration

When you need to change the storage schema (add fields, new stores, etc.), follow these steps:

### Step 1: Determine Migration Type

- **Structural changes** (new stores, new indexes): Require incrementing `CURRENT_DB_VERSION`
- **Data transformations** (add field to existing records, compute values): Only need `CURRENT_SCHEMA_VERSION` bump

### Step 2: Update Version Constants

In `src/services/storage/migrations.ts`:

```typescript
// If adding new stores or indexes:
export const CURRENT_DB_VERSION = 3;  // Increment this

// Always increment for any migration:
export const CURRENT_SCHEMA_VERSION = 3;  // Increment this
```

### Step 3: Add Migration to Registry

Add a new entry to the `migrations` array in `src/services/storage/migrations.ts`:

```typescript
export const migrations: Migration[] = [
  // ... existing migrations ...

  {
    version: 3,  // Must match CURRENT_SCHEMA_VERSION
    description: 'Brief description of what this migration does',

    // Optional: structural changes (runs during IndexedDB upgrade)
    structural: (db) => {
      if (!db.objectStoreNames.contains('newStoreName')) {
        db.createObjectStore('newStoreName', { keyPath: 'id' });
      }
    },

    // Optional: data transformation (runs after DB is open)
    transform: async (db) => {
      // Example: add a new field to all existing records
      const tx = db.transaction('projects', 'readwrite');
      const store = tx.objectStore('projects');
      const allRecords = await store.getAll();

      for (const record of allRecords) {
        if (record.newField === undefined) {
          record.newField = 'default value';
          await store.put(record);
        }
      }
    },
  },
];
```

### Step 4: Update Types (if needed)

If adding new stores or fields, update the type definitions in `src/services/storage/types.ts`:

```typescript
// Add store name to STORES constant
export const STORES = {
  META: '_meta',
  PROJECTS: 'projects',
  USER_SETTINGS: 'userSettings',
  NEW_STORE: 'newStoreName',  // Add new store
} as const;

// Add interface for new data types
export interface NewStoreEntry {
  id: string;
  // ... fields
}
```

### Step 5: Test the Migration

1. **Fresh install**: Delete IndexedDB in browser DevTools, reload app
2. **Upgrade path**: Keep existing DB, reload app, verify data preserved
3. **Check migration history**: In browser console:
   ```javascript
   const db = await indexedDB.open('quarto-hub');
   // Check _meta store for schema version and migration history
   ```

## Migration Best Practices

1. **Migrations must be idempotent**: Safe to run multiple times
2. **Never modify existing migrations**: Only add new ones
3. **Keep migrations fast**: Users wait during migration
4. **Handle missing data gracefully**: Old records may lack new fields
5. **Test both fresh install and upgrade paths**

## Database Stores

| Store | Key | Purpose |
|-------|-----|---------|
| `projects` | `id` (UUID) | Project connection info (sync server, index doc ID) |
| `userSettings` | `key` (singleton: `'identity'`) | User identity for presence features |
| `_meta` | `key` (singleton: `'schema'`) | Schema version and migration history |

## Export/Import Format

The `exportData()` function produces JSON with schema version for forward compatibility:

```typescript
interface ExportData {
  schemaVersion: number;
  exportedAt: string;
  projects: ProjectEntry[];
  userSettings?: UserSettings;
}
```

The `importData()` function handles both:
- Old format: Plain array of projects (pre-migration system)
- New format: ExportData object with version

## Troubleshooting

### Migration failed
Check browser console for error details. The `_meta` store records `lastMigrationError` with details. Fix the issue and reload - the migration will retry.

### Schema version mismatch
If `_meta.version` doesn't match `CURRENT_SCHEMA_VERSION`, pending migrations will run on next database access.

### Need to reset database
In browser DevTools → Application → IndexedDB → Delete `quarto-hub` database. All local data will be lost (projects list, user settings).
