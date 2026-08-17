/**
 * WASM-JS Bridge for Cache Operations
 *
 * This module provides cache functions backed by IndexedDB, called from Rust
 * WASM code via wasm-bindgen. Used for persistent caching of expensive computed
 * results (SASS compilation, metadata parsing, etc.).
 *
 * The functions are imported by quarto-system-runtime/src/wasm.rs using:
 *
 *   #[wasm_bindgen(raw_module = "/src/wasm-js-bridge/cache.js")]
 *
 * Key design decisions:
 * - Lazy initialization: IndexedDB database is opened on first access
 * - LRU eviction: oldest-accessed entries are evicted when limits are exceeded
 * - Composite key format: "<namespace>:<key>" for flat object store
 * - Touch-on-read: accessing a cached entry updates its timestamp (true LRU)
 */

const DB_NAME = "quarto-cache";
const DB_VERSION = 1;
const STORE_NAME = "cache";

/** Maximum number of entries before eviction triggers. */
export const MAX_ENTRIES = 200;

/** Maximum total size in bytes before eviction triggers (50MB). */
export const MAX_TOTAL_SIZE = 50 * 1024 * 1024;

/**
 * Ephemeral in-memory mode (bd-91mdd056).
 *
 * A host page sets `globalThis.__Q2_EPHEMERAL_STORAGE__ = true` before
 * the WASM first touches the cache when the session is ephemeral — q2
 * preview serves every session from a fresh origin (a random loopback
 * port), so a persisted `quarto-cache` database would never be read
 * again and just accumulates across sessions. In this mode everything
 * lives in a module-level Map with the same LRU semantics (touch-on-
 * read, same eviction limits); the store dies with the page.
 *
 * The flag is a runtime global rather than a build-time
 * `import.meta.env` read so this module stays byte-identical across the
 * viewer/editor builds — the q2 preview embed's asset dedupe and
 * live-share manifest hashes depend on the WASM glue chunk not
 * diverging.
 *
 * @type {Map<string, {namespace: string, key: string, value: Uint8Array, timestamp: number, size: number}>}
 */
const memoryStore = new Map();

function ephemeralMode() {
  return globalThis.__Q2_EPHEMERAL_STORAGE__ === true;
}

/** @type {IDBDatabase | null} */
let db = null;

/** @type {Promise<IDBDatabase> | null} */
let dbOpenPromise = null;

/**
 * Lazy-open the IndexedDB database.
 *
 * The database is opened once and reused for all subsequent operations.
 * If the database doesn't exist, it is created with a single object store
 * and a timestamp index for LRU eviction.
 *
 * @returns {Promise<IDBDatabase>}
 */
function openDb() {
  if (db) return Promise.resolve(db);
  if (dbOpenPromise) return dbOpenPromise;

  dbOpenPromise = new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);

    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains(STORE_NAME)) {
        const store = database.createObjectStore(STORE_NAME);
        store.createIndex("timestamp", "timestamp", { unique: false });
      }
    };

    request.onsuccess = () => {
      db = request.result;
      resolve(db);
    };

    request.onerror = () => {
      dbOpenPromise = null;
      reject(new Error(`Failed to open IndexedDB "${DB_NAME}": ${request.error?.message}`));
    };
  });

  return dbOpenPromise;
}

/**
 * Build the composite key for the object store.
 *
 * @param {string} namespace
 * @param {string} key
 * @returns {string}
 */
function compositeKey(namespace, key) {
  return `${namespace}:${key}`;
}

/**
 * Get a cached value by namespace and key.
 *
 * On a cache hit, the entry's timestamp is updated to the current time
 * (touch-on-read) so that actively-used entries survive LRU eviction.
 *
 * @param {string} namespace - Cache namespace (e.g. "sass", "metadata")
 * @param {string} key - Cache key (typically a hex-encoded hash)
 * @returns {Promise<Uint8Array | null>} The cached bytes, or null on miss
 */
export async function jsCacheGet(namespace, key) {
  if (ephemeralMode()) {
    const record = memoryStore.get(compositeKey(namespace, key));
    if (record == null) return null;
    // Touch-on-read, mirroring the IndexedDB path.
    record.timestamp = Date.now();
    return record.value;
  }
  const database = await openDb();
  const ck = compositeKey(namespace, key);

  return new Promise((resolve, reject) => {
    const tx = database.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const request = store.get(ck);

    request.onsuccess = () => {
      const record = request.result;
      if (record == null) {
        resolve(null);
      } else {
        // Touch-on-read: update timestamp so this entry survives LRU eviction
        record.timestamp = Date.now();
        store.put(record, ck);
        resolve(record.value);
      }
    };

    request.onerror = () => {
      reject(new Error(`Cache get failed: ${request.error?.message}`));
    };
  });
}

/**
 * Store a value in the cache, then evict oldest entries if limits are exceeded.
 *
 * @param {string} namespace - Cache namespace
 * @param {string} key - Cache key
 * @param {Uint8Array} value - The bytes to cache
 * @returns {Promise<void>}
 */
export async function jsCacheSet(namespace, key, value) {
  if (ephemeralMode()) {
    memoryStore.set(compositeKey(namespace, key), {
      namespace,
      key,
      value,
      timestamp: Date.now(),
      size: value.length,
    });
    evictMemoryIfNeeded();
    return;
  }
  const database = await openDb();

  // Store the entry
  await new Promise((resolve, reject) => {
    const tx = database.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const record = {
      namespace,
      key,
      value,
      timestamp: Date.now(),
      size: value.length,
    };
    const request = store.put(record, compositeKey(namespace, key));

    request.onsuccess = () => resolve();
    request.onerror = () =>
      reject(new Error(`Cache set failed: ${request.error?.message}`));
  });

  // Evict if over limits (best-effort, errors are non-fatal)
  try {
    await evictIfNeeded(database);
  } catch {
    // Eviction errors are non-fatal
  }
}

/**
 * Evict oldest entries (by timestamp) until both entry count and total size
 * are within limits. Eviction is global across all namespaces.
 *
 * @param {IDBDatabase} database
 * @returns {Promise<void>}
 */
async function evictIfNeeded(database) {
  // First, get count and total size
  const stats = await new Promise((resolve, reject) => {
    const tx = database.transaction(STORE_NAME, "readonly");
    const store = tx.objectStore(STORE_NAME);
    let count = 0;
    let totalSize = 0;
    const request = store.openCursor();

    request.onsuccess = () => {
      const cursor = request.result;
      if (cursor) {
        count++;
        totalSize += cursor.value.size || 0;
        cursor.continue();
      }
    };

    tx.oncomplete = () => resolve({ count, totalSize });
    tx.onerror = () => reject(tx.error);
  });

  if (stats.count <= MAX_ENTRIES && stats.totalSize <= MAX_TOTAL_SIZE) {
    return;
  }

  // Evict oldest entries using timestamp index
  let { count, totalSize } = stats;
  await new Promise((resolve, reject) => {
    const tx = database.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const index = store.index("timestamp");
    const request = index.openCursor(); // ascending = oldest first

    request.onsuccess = () => {
      const cursor = request.result;
      if (cursor && (count > MAX_ENTRIES || totalSize > MAX_TOTAL_SIZE)) {
        const entrySize = cursor.value.size || 0;
        cursor.delete();
        count--;
        totalSize -= entrySize;
        cursor.continue();
      }
    };

    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

/**
 * Evict oldest entries (by timestamp) from the in-memory store until
 * both entry count and total size are within limits. Mirrors the
 * IndexedDB path's global-across-namespaces eviction.
 */
function evictMemoryIfNeeded() {
  let totalSize = 0;
  for (const record of memoryStore.values()) {
    totalSize += record.size || 0;
  }
  if (memoryStore.size <= MAX_ENTRIES && totalSize <= MAX_TOTAL_SIZE) {
    return;
  }

  // Oldest first, matching the IDB path's ascending timestamp index.
  const byAge = [...memoryStore.entries()].sort(
    (a, b) => a[1].timestamp - b[1].timestamp,
  );
  let count = memoryStore.size;
  for (const [ck, record] of byAge) {
    if (count <= MAX_ENTRIES && totalSize <= MAX_TOTAL_SIZE) break;
    memoryStore.delete(ck);
    count--;
    totalSize -= record.size || 0;
  }
}

/**
 * Delete a cached value by namespace and key.
 *
 * No-op if the key does not exist.
 *
 * @param {string} namespace - Cache namespace
 * @param {string} key - Cache key
 * @returns {Promise<void>}
 */
export async function jsCacheDelete(namespace, key) {
  if (ephemeralMode()) {
    memoryStore.delete(compositeKey(namespace, key));
    return;
  }
  const database = await openDb();

  return new Promise((resolve, reject) => {
    const tx = database.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const request = store.delete(compositeKey(namespace, key));

    request.onsuccess = () => resolve();
    request.onerror = () =>
      reject(new Error(`Cache delete failed: ${request.error?.message}`));
  });
}

/**
 * Clear all cached values in a namespace.
 *
 * Iterates over all entries and removes those matching the namespace prefix.
 *
 * @param {string} namespace - Cache namespace to clear
 * @returns {Promise<void>}
 */
export async function jsCacheClearNamespace(namespace) {
  const prefix = `${namespace}:`;
  if (ephemeralMode()) {
    for (const ck of memoryStore.keys()) {
      if (ck.startsWith(prefix)) memoryStore.delete(ck);
    }
    return;
  }
  const database = await openDb();

  return new Promise((resolve, reject) => {
    const tx = database.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const request = store.openCursor();

    request.onsuccess = () => {
      const cursor = request.result;
      if (cursor) {
        if (typeof cursor.key === "string" && cursor.key.startsWith(prefix)) {
          cursor.delete();
        }
        cursor.continue();
      }
    };

    tx.oncomplete = () => resolve();
    tx.onerror = () =>
      reject(new Error(`Cache clear namespace failed: ${tx.error?.message}`));
  });
}

/**
 * Reset the module-level database handle and clear the in-memory store.
 *
 * Exported for testing only — allows tests to delete the database and
 * re-open a fresh one without stale handles.
 */
export function _resetDbHandle() {
  memoryStore.clear();
  if (db) {
    db.close();
    db = null;
  }
  dbOpenPromise = null;
}
