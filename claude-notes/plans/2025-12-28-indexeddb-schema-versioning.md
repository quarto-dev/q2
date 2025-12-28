# IndexedDB Schema Versioning and Migration System

**Beads Issue:** `k-ifux` - Implement IndexedDB schema versioning and migration system
**Related:** `k-evpj` - Presence features (requires user identity storage)

## Problem Statement

The current IndexedDB implementation has a `DB_VERSION = 1` constant but lacks:

1. **Explicit schema version tracking** - No way to know what data schema version is stored
2. **Data migration logic** - Only handles initial store creation, not data transformation
3. **Migration history** - No record of what migrations have been applied
4. **Error recovery** - No handling for failed migrations

When we add user identity for presence features, and for future schema changes, we need a robust system to:
- Detect outdated schemas
- Apply migrations in order
- Handle failures gracefully
- Preserve existing data (project IDs, settings)

## Current Implementation Analysis

```typescript
// projectStorage.ts - current state
const DB_NAME = 'quarto-hub';
const DB_VERSION = 1;
const STORE_NAME = 'projects';

dbPromise = openDB(DB_NAME, DB_VERSION, {
  upgrade(db) {
    // Only handles initial creation, not migration
    if (!db.objectStoreNames.contains(STORE_NAME)) {
      const store = db.createObjectStore(STORE_NAME, { keyPath: 'id' });
      store.createIndex('indexDocId', 'indexDocId', { unique: true });
      store.createIndex('lastAccessed', 'lastAccessed');
    }
  },
});
```

**Limitations:**
- IndexedDB's `upgrade` runs synchronously during `openDB`
- No way to run async operations (e.g., data transformation)
- Version is implicit in the code, not explicit in the data
- Bumping `DB_VERSION` triggers upgrade but we have no migration logic

## Proposed Architecture

### Design Principles

1. **Separate concerns**: Structure changes (stores/indexes) vs. data transformation
2. **Explicit versioning**: Store schema version as data, not just code constant
3. **Idempotent migrations**: Safe to re-run if partially completed
4. **Forward-only**: No downgrade support (simplifies implementation)
5. **Atomic where possible**: Use transactions to prevent partial state

### Two-Layer Migration System

```
┌─────────────────────────────────────────────────────────────────┐
│                     Application Startup                          │
└─────────────────────────────────┬───────────────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────┐
│                  Layer 1: IndexedDB Native Upgrade               │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │ • Create/delete object stores                              │  │
│  │ • Create/delete indexes                                    │  │
│  │ • Runs synchronously during openDB()                       │  │
│  │ • Triggered by DB_VERSION change                           │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────┬───────────────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────┐
│                Layer 2: Application-Level Migrations             │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │ • Read schemaVersion from _meta store                      │  │
│  │ • Run migrations sequentially: v1→v2→v3→...                │  │
│  │ • Async operations allowed                                 │  │
│  │ • Data transformation within stores                        │  │
│  │ • Update schemaVersion after each migration                │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────┬───────────────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Application Ready                           │
└─────────────────────────────────────────────────────────────────┘
```

### Schema Metadata Store

Create a `_meta` object store to track schema state:

```typescript
interface SchemaMeta {
  key: 'schema';              // Singleton key
  version: number;            // Current schema version (e.g., 1, 2, 3)
  migrationsApplied: {        // History of applied migrations
    version: number;
    appliedAt: string;        // ISO timestamp
    durationMs: number;
  }[];
  lastMigrationError?: {      // Last error if migration failed
    version: number;
    error: string;
    occurredAt: string;
  };
}
```

### Migration Definition Interface

```typescript
interface Migration {
  version: number;           // Target version after this migration
  description: string;       // Human-readable description

  // Structural changes - run in IndexedDB upgrade callback
  structural?: (db: IDBDatabase, tx: IDBTransaction) => void;

  // Data transformation - run after DB is open
  transform?: (db: IDBPDatabase) => Promise<void>;
}

// Example migration registry
const migrations: Migration[] = [
  {
    version: 2,
    description: 'Add userSettings store for presence identity',
    structural: (db) => {
      db.createObjectStore('userSettings', { keyPath: 'key' });
    },
  },
  {
    version: 3,
    description: 'Add color field to userSettings',
    transform: async (db) => {
      const settings = await db.get('userSettings', 'identity');
      if (settings && !settings.userColor) {
        settings.userColor = generateColorFromId(settings.userId);
        await db.put('userSettings', settings);
      }
    },
  },
];
```

## Implementation Plan

### Phase 1: Core Migration Infrastructure

#### 1.1 Create Migration Types
Create `hub-client/src/services/storage/types.ts`:

```typescript
export interface SchemaMeta {
  key: 'schema';
  version: number;
  migrationsApplied: MigrationRecord[];
  lastMigrationError?: MigrationError;
}

export interface MigrationRecord {
  version: number;
  appliedAt: string;
  durationMs: number;
}

export interface MigrationError {
  version: number;
  error: string;
  occurredAt: string;
}

export interface Migration {
  version: number;
  description: string;
  structural?: (db: IDBDatabase, transaction: IDBTransaction) => void;
  transform?: (db: IDBPDatabase) => Promise<void>;
}
```

#### 1.2 Create Migration Registry
Create `hub-client/src/services/storage/migrations.ts`:

```typescript
import type { Migration } from './types';

export const CURRENT_SCHEMA_VERSION = 1;

export const migrations: Migration[] = [
  // Migrations will be added here as the schema evolves
  // Version 1 is the baseline (current state)
];

export function getMigrationsFrom(fromVersion: number): Migration[] {
  return migrations
    .filter(m => m.version > fromVersion)
    .sort((a, b) => a.version - b.version);
}
```

#### 1.3 Create Migration Runner
Create `hub-client/src/services/storage/migrationRunner.ts`:

```typescript
export async function runMigrations(db: IDBPDatabase): Promise<void> {
  const meta = await getOrCreateSchemaMeta(db);
  const pendingMigrations = getMigrationsFrom(meta.version);

  for (const migration of pendingMigrations) {
    const startTime = Date.now();
    try {
      if (migration.transform) {
        await migration.transform(db);
      }
      await recordMigrationSuccess(db, migration.version, Date.now() - startTime);
    } catch (error) {
      await recordMigrationError(db, migration.version, error);
      throw new MigrationError(migration.version, error);
    }
  }
}
```

#### 1.4 Refactor Database Initialization
Update `hub-client/src/services/projectStorage.ts`:

```typescript
import { runMigrations } from './storage/migrationRunner';
import { getStructuralMigrations, CURRENT_DB_VERSION } from './storage/migrations';

async function getDb(): Promise<IDBPDatabase> {
  if (!dbPromise) {
    dbPromise = (async () => {
      // Phase 1: Open DB with structural migrations
      const db = await openDB(DB_NAME, CURRENT_DB_VERSION, {
        upgrade(db, oldVersion, newVersion, transaction) {
          // Always create _meta store if missing
          if (!db.objectStoreNames.contains('_meta')) {
            db.createObjectStore('_meta', { keyPath: 'key' });
          }

          // Run structural migrations
          const structuralMigrations = getStructuralMigrations(oldVersion);
          for (const migration of structuralMigrations) {
            if (migration.structural) {
              migration.structural(db, transaction);
            }
          }
        },
      });

      // Phase 2: Run data transformation migrations
      await runMigrations(db);

      return db;
    })();
  }
  return dbPromise;
}
```

### Phase 2: User Identity Storage

#### 2.1 Define User Settings Schema

```typescript
// types.ts addition
export interface UserSettings {
  key: 'identity';
  userId: string;
  userName: string;
  userColor: string;
  createdAt: string;
  updatedAt: string;
}
```

#### 2.2 Create Migration for User Settings Store

```typescript
// migrations.ts addition
export const migrations: Migration[] = [
  {
    version: 2,
    description: 'Add userSettings store for presence identity',
    structural: (db) => {
      if (!db.objectStoreNames.contains('userSettings')) {
        db.createObjectStore('userSettings', { keyPath: 'key' });
      }
    },
    transform: async (db) => {
      // Initialize default user identity if not exists
      const existing = await db.get('userSettings', 'identity');
      if (!existing) {
        const userId = crypto.randomUUID();
        await db.put('userSettings', {
          key: 'identity',
          userId,
          userName: generateAnonymousName(),
          userColor: generateColorFromId(userId),
          createdAt: new Date().toISOString(),
          updatedAt: new Date().toISOString(),
        });
      }
    },
  },
];
```

#### 2.3 Create User Settings Service
Create `hub-client/src/services/userSettings.ts`:

```typescript
export async function getUserIdentity(): Promise<UserSettings>;
export async function updateUserName(name: string): Promise<void>;
export async function updateUserColor(color: string): Promise<void>;
export async function resetUserIdentity(): Promise<UserSettings>;
```

### Phase 3: Error Handling and Recovery

#### 3.1 Migration Error UI
- Show user-friendly error when migration fails
- Offer options:
  - **Retry**: Attempt migration again
  - **Export & Reset**: Export data as JSON, clear DB, reimport
  - **Report**: Copy diagnostic info for bug report

#### 3.2 Graceful Degradation
- If migration fails, allow read-only access to existing data
- Disable features that require new schema

#### 3.3 Manual Export/Import Enhancement
- Extend existing `exportProjects()` / `importProjects()`
- Include schema version in export:
  ```typescript
  interface ExportData {
    schemaVersion: number;
    exportedAt: string;
    projects: ProjectEntry[];
    userSettings?: UserSettings;
  }
  ```
- Validate import against current schema
- Transform old format during import if needed

### Phase 4: Testing Infrastructure

#### 4.1 Migration Tests
```typescript
describe('migrations', () => {
  it('should migrate from v1 to v2', async () => {
    // Create v1 database with test data
    // Run migration
    // Verify v2 schema and data
  });

  it('should handle interrupted migration', async () => {
    // Simulate failure mid-migration
    // Verify recovery on retry
  });
});
```

#### 4.2 Browser Testing
- Test in Chrome, Firefox, Safari
- Test with existing quarto-hub databases
- Test fresh install path

## File Changes Summary

| File | Change Type | Description |
|------|-------------|-------------|
| `src/services/storage/types.ts` | Create | Schema and migration type definitions |
| `src/services/storage/migrations.ts` | Create | Migration registry and helpers |
| `src/services/storage/migrationRunner.ts` | Create | Migration execution logic |
| `src/services/storage/utils.ts` | Create | Color generation, name generation |
| `src/services/projectStorage.ts` | Modify | Integrate migration system |
| `src/services/userSettings.ts` | Create | User identity CRUD operations |
| `src/types/project.ts` | Modify | Add UserSettings interface |
| `src/components/MigrationError.tsx` | Create | Error UI component |

## Migration Strategy for Existing Users

When users with existing `quarto-hub` IndexedDB visit the updated app:

```
1. openDB() with new DB_VERSION triggers upgrade callback
   ├─ _meta store created (if missing)
   └─ userSettings store created

2. runMigrations() executes
   ├─ Reads _meta.version (null → defaults to 1)
   ├─ Runs v1→v2 migration
   │   └─ Creates default user identity
   └─ Updates _meta.version = 2

3. App initializes normally
   └─ Existing projects preserved
```

## Design Decisions

1. **Migration Rollback**: **No rollback support** - forward-only migrations.
   - Simplifies implementation significantly
   - If a migration causes issues, we ship a new forward migration to fix it
   - Good error recovery and data export provide safety net

2. **Schema Version in Export**: **Yes, include version** in exported JSON.
   - Enables smart import with transformation for older exports
   - Future-proofs the export/import workflow

3. **Multiple Browser Tabs**: Show "Please reload" banner in other tabs when schema changes.
   - Schema migrations expected to be rare
   - Simple UX is sufficient

4. **Progress UI**: **Not needed** - migrations expected to be very fast.
   - Can add later if a migration ever requires significant data processing

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Migration corrupts data | High | Backup to localStorage before migration |
| Migration fails midway | Medium | Idempotent migrations, clear error state |
| IndexedDB quota exceeded | Low | Warn user, offer cleanup |
| Browser compatibility | Low | Test across browsers, feature detection |

## Success Criteria

1. Existing users' project lists preserved after update
2. New userSettings store created and initialized
3. Migration errors handled gracefully with recovery path
4. Schema version tracked and queryable
5. Foundation ready for future migrations

## References

- [idb library documentation](https://github.com/jakearchibald/idb)
- [IndexedDB versioning spec](https://w3c.github.io/IndexedDB/#opening)
- [MDN: Using IndexedDB](https://developer.mozilla.org/en-US/docs/Web/API/IndexedDB_API/Using_IndexedDB)
