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
 * - Simple key-value store: no LRU eviction for v1
 * - Composite key format: "<namespace>:<key>" for flat object store
 */

const DB_NAME = "quarto-cache";
const DB_VERSION = 1;
const STORE_NAME = "cache";

/** @type {IDBDatabase | null} */
let db = null;

/** @type {Promise<IDBDatabase> | null} */
let dbOpenPromise = null;

/**
 * Lazy-open the IndexedDB database.
 *
 * The database is opened once and reused for all subsequent operations.
 * If the database doesn't exist, it is created with a single object store.
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
        database.createObjectStore(STORE_NAME);
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
 * @param {string} namespace - Cache namespace (e.g. "sass", "metadata")
 * @param {string} key - Cache key (typically a hex-encoded hash)
 * @returns {Promise<Uint8Array | null>} The cached bytes, or null on miss
 */
export async function jsCacheGet(namespace, key) {
  const database = await openDb();

  return new Promise((resolve, reject) => {
    const tx = database.transaction(STORE_NAME, "readonly");
    const store = tx.objectStore(STORE_NAME);
    const request = store.get(compositeKey(namespace, key));

    request.onsuccess = () => {
      const record = request.result;
      if (record == null) {
        resolve(null);
      } else {
        resolve(record.value);
      }
    };

    request.onerror = () => {
      reject(new Error(`Cache get failed: ${request.error?.message}`));
    };
  });
}

/**
 * Store a value in the cache.
 *
 * Overwrites any existing entry with the same namespace+key.
 *
 * @param {string} namespace - Cache namespace
 * @param {string} key - Cache key
 * @param {Uint8Array} value - The bytes to cache
 * @returns {Promise<void>}
 */
export async function jsCacheSet(namespace, key, value) {
  const database = await openDb();

  return new Promise((resolve, reject) => {
    const tx = database.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const record = {
      namespace,
      key,
      value,
      timestamp: Date.now(),
    };
    const request = store.put(record, compositeKey(namespace, key));

    request.onsuccess = () => {
      resolve();
    };

    request.onerror = () => {
      reject(new Error(`Cache set failed: ${request.error?.message}`));
    };
  });
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
  const database = await openDb();

  return new Promise((resolve, reject) => {
    const tx = database.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const request = store.delete(compositeKey(namespace, key));

    request.onsuccess = () => {
      resolve();
    };

    request.onerror = () => {
      reject(new Error(`Cache delete failed: ${request.error?.message}`));
    };
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
  const database = await openDb();
  const prefix = `${namespace}:`;

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

    tx.oncomplete = () => {
      resolve();
    };

    tx.onerror = () => {
      reject(new Error(`Cache clear namespace failed: ${tx.error?.message}`));
    };
  });
}

/**
 * Reset the module-level database handle.
 *
 * Exported for testing only — allows tests to delete the database and
 * re-open a fresh one without stale handles.
 */
export function _resetDbHandle() {
  if (db) {
    db.close();
    db = null;
  }
  dbOpenPromise = null;
}
