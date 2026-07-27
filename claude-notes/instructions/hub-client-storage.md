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
| `projects` | `id` (UUID) | Legacy project connection info (sync server, index doc ID). Kept as a safety net / migration source; superseded by the synced project-set document. |
| `projectSet` | `key` | Pointers to the synced Automerge document(s). Holds the legacy singleton `'projectSet'` pointer **and** the `'collections'` pointer array (see Migration History v4/v5). |
| `userSettings` | `key` (singleton: `'identity'`) | User identity for presence features |
| `_meta` | `key` (singleton: `'schema'`) | Schema version and migration history |

## Migration History

| Schema | DB ver | Kind | What it does |
|-------:|-------:|------|--------------|
| v2 | 2 | structural + transform | Add `_meta` and `userSettings` stores; seed a default user identity. |
| v3 | 3 | no-op | Formerly a `sassCache` store; caching moved to the `quarto-cache` DB. Kept so existing v3 databases don't mismatch. |
| v4 | 4 | structural | Add the `projectSet` store. The project list moves from the `projects` store into a synced Automerge **ProjectSetDocument**; IndexedDB keeps only a singleton `'projectSet'` pointer `{ projectSetDocId, syncServer }` to it. |
| v5 | 4 | transform-only | **Collections.** The root goes from a single project set to an array of collections (each collection is its own ProjectSetDocument). The `'projectSet'` singleton pointer is converted into a one-element `'collections'` array (`{ key:'collections', collections:[{ projectSetDocId, syncServer }] }`), stored in the same `projectSet` store. The legacy singleton is **retained untouched** as a safety net. See `migratePointerToCollections` in `migrations.ts` and the plan `claude-notes/plans/2026-07-10-collections-as-project-sets.md`. |

**Note on v5 (transform-only):** it bumps `CURRENT_SCHEMA_VERSION` (5) but **not** `CURRENT_DB_VERSION` (stays 4), because it adds no store/index — it only rewrites a record. The migration runner keys off the stored schema version, so it applies the v5 `transform` even though the IndexedDB version is unchanged. `getCollectionPointers` in `projectSetStorage.ts` also self-heals (runs the same conversion lazily) as defense in depth. Tests: `migrations.test.ts` and the collection-pointer suite in `projectSetStorage.test.ts`.

**Second migration (app-level, not IndexedDB):** legacy per-browser "collections" that lived in `localStorage` under `qh-collections-v1` (the pre-synced exploration) are converted into real synced collection documents on first load by `migrateLocalCollections` in `hooks/useCollectionSets.ts`; the original JSON is preserved under `qh-collections-v1-migrated` and never re-imported.

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
