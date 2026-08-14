/**
 * In-memory HubDatabase facade for ephemeral storage mode (bd-sw4xy1vw).
 *
 * Backs `getDb()` when the q2 preview embed build runs with
 * VITE_EPHEMERAL_STORAGE=1. Supports exactly the IDB surface the
 * consumer modules (projectStorage, userSettings, projectSetStorage)
 * and the migration helpers use — `get`/`put`/`delete`,
 * `transaction→objectStore→index→{get,getAll}`,
 * `objectStoreNames.contains`, and `close` — and behaves like the real
 * database for those operations. Anything outside that surface fails
 * loudly rather than silently diverging from IndexedDB semantics.
 *
 * Data dies with the page: that is the point. Each `q2 preview` session
 * is a fresh origin, so a persisted copy would never be read again.
 */

import type { IDBPDatabase } from 'idb';
import { STORES } from './types';
import type { SchemaMeta } from './types';
import { CURRENT_SCHEMA_VERSION } from './migrations';

/**
 * Key paths per store, mirroring the structural migrations
 * (`projects` keyed by `id`; the singleton stores keyed by `key`).
 */
const KEY_PATHS: Record<string, string> = {
  [STORES.PROJECTS]: 'id',
  [STORES.META]: 'key',
  [STORES.USER_SETTINGS]: 'key',
  [STORES.PROJECT_SET]: 'key',
};

/** The projects store's secondary indexes, mapped to their key paths. */
const INDEX_KEY_PATHS: Record<string, string> = {
  indexDocId: 'indexDocId',
  lastAccessed: 'lastAccessed',
};

type RecordValue = Record<string, unknown>;

class MemoryIndex {
  private readonly records: Map<string, unknown>;
  private readonly name: string;

  constructor(records: Map<string, unknown>, name: string) {
    this.records = records;
    this.name = name;
  }

  private keyPath(): string {
    const keyPath = INDEX_KEY_PATHS[this.name];
    if (!keyPath) {
      throw new Error(`memoryDb: unknown index '${this.name}'`);
    }
    return keyPath;
  }

  async get(key: string): Promise<unknown> {
    const field = this.keyPath();
    for (const value of this.records.values()) {
      if ((value as RecordValue)[field] === key) return value;
    }
    return undefined;
  }

  async getAll(): Promise<unknown[]> {
    const field = this.keyPath();
    // IDB cursors iterate an index in ascending key order.
    return [...this.records.values()].sort((a, b) => {
      const av = String((a as RecordValue)[field]);
      const bv = String((b as RecordValue)[field]);
      return av < bv ? -1 : av > bv ? 1 : 0;
    });
  }
}

class MemoryHubDatabase {
  private readonly stores = new Map<string, Map<string, unknown>>();

  constructor() {
    for (const store of Object.values(STORES)) {
      this.stores.set(store, new Map());
    }
    // Migrations never run in ephemeral mode; pre-seed the schema meta
    // so getSchemaVersion() reports current instead of baseline.
    const meta: SchemaMeta = {
      key: 'schema',
      version: CURRENT_SCHEMA_VERSION,
      migrationsApplied: [],
    };
    this.stores.get(STORES.META)!.set('schema', meta);
  }

  readonly objectStoreNames = {
    contains: (name: string): boolean => this.stores.has(name),
  };

  private recordsFor(store: string): Map<string, unknown> {
    const records = this.stores.get(store);
    if (!records) {
      throw new Error(`memoryDb: unknown store '${store}'`);
    }
    return records;
  }

  private keyFor(store: string, value: unknown): string {
    const keyPath = KEY_PATHS[store];
    const key = (value as RecordValue | null)?.[keyPath];
    if (typeof key !== 'string') {
      throw new Error(`memoryDb: '${store}' record missing keyPath '${keyPath}'`);
    }
    return key;
  }

  async get(store: string, key: string): Promise<unknown> {
    return this.recordsFor(store).get(key);
  }

  async put(store: string, value: unknown): Promise<void> {
    this.recordsFor(store).set(this.keyFor(store, value), value);
  }

  async delete(store: string, key: string): Promise<void> {
    this.recordsFor(store).delete(key);
  }

  transaction(store: string, _mode: 'readonly' | 'readwrite'): {
    objectStore: (name: string) => { index: (name: string) => MemoryIndex };
  } {
    const records = this.recordsFor(store);
    return {
      objectStore: () => ({
        index: (name: string) => new MemoryIndex(records, name),
      }),
    };
  }

  /** Matches IDB: closing the connection does not delete data. */
  close(): void {}
}

/**
 * Create a fresh, empty in-memory database. The cast is the seam: the
 * facade implements only the subset of `IDBPDatabase` the consumers
 * use (see module docstring).
 */
export function createMemoryHubDatabase(): IDBPDatabase {
  return new MemoryHubDatabase() as unknown as IDBPDatabase;
}
