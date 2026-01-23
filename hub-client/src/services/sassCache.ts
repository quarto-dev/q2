/**
 * SASS Compilation Cache Manager
 *
 * Provides caching for compiled SCSS to CSS with LRU eviction.
 * Uses a storage backend interface for testability - IndexedDB in production,
 * in-memory Map for unit tests.
 */

import { openDB } from 'idb';
import type { IDBPDatabase } from 'idb';
import type { SassCacheEntry } from './storage/types';
import { DB_NAME, STORES, CURRENT_DB_VERSION } from './storage';

/**
 * Configuration for the SASS cache.
 */
export interface SassCacheConfig {
  /** Maximum total size of cached CSS in bytes (default: 50MB) */
  maxSizeBytes: number;
  /** Maximum number of cache entries (default: 1000) */
  maxEntries: number;
}

/**
 * Statistics about the cache state.
 */
export interface SassCacheStats {
  /** Number of entries in the cache */
  entryCount: number;
  /** Total size of all cached CSS in bytes */
  totalSizeBytes: number;
  /** Oldest entry timestamp (ms) */
  oldestEntry: number | null;
  /** Newest entry timestamp (ms) */
  newestEntry: number | null;
}

/**
 * Default cache configuration.
 */
export const DEFAULT_CACHE_CONFIG: SassCacheConfig = {
  maxSizeBytes: 50 * 1024 * 1024, // 50MB
  maxEntries: 1000,
};

// ============================================================================
// Storage Backend Interface
// ============================================================================

/**
 * Storage backend interface for the SASS cache.
 *
 * This abstraction allows swapping IndexedDB (production) for an in-memory
 * implementation (testing).
 */
export interface SassCacheStorage {
  /** Get an entry by key */
  get(key: string): Promise<SassCacheEntry | undefined>;
  /** Store an entry */
  put(entry: SassCacheEntry): Promise<void>;
  /** Delete an entry by key */
  delete(key: string): Promise<void>;
  /** Get all entries sorted by lastUsed (oldest first) */
  getAllSortedByLastUsed(): Promise<SassCacheEntry[]>;
  /** Clear all entries */
  clear(): Promise<void>;
  /** Check if storage is available */
  isAvailable(): Promise<boolean>;
}

// ============================================================================
// IndexedDB Storage Backend (Production)
// ============================================================================

/**
 * IndexedDB-backed storage for the SASS cache.
 *
 * Used in production for persistent caching across sessions.
 */
export class IndexedDBCacheStorage implements SassCacheStorage {
  private dbPromise: Promise<IDBPDatabase> | null = null;

  private async getDb(): Promise<IDBPDatabase> {
    if (!this.dbPromise) {
      this.dbPromise = openDB(DB_NAME, CURRENT_DB_VERSION);
    }
    return this.dbPromise;
  }

  async isAvailable(): Promise<boolean> {
    try {
      const db = await this.getDb();
      return db.objectStoreNames.contains(STORES.SASS_CACHE);
    } catch {
      return false;
    }
  }

  async get(key: string): Promise<SassCacheEntry | undefined> {
    try {
      const db = await this.getDb();
      if (!db.objectStoreNames.contains(STORES.SASS_CACHE)) {
        return undefined;
      }
      return (await db.get(STORES.SASS_CACHE, key)) as SassCacheEntry | undefined;
    } catch {
      return undefined;
    }
  }

  async put(entry: SassCacheEntry): Promise<void> {
    try {
      const db = await this.getDb();
      if (!db.objectStoreNames.contains(STORES.SASS_CACHE)) {
        return;
      }
      await db.put(STORES.SASS_CACHE, entry);
    } catch (error) {
      console.warn('SASS cache put failed:', error);
    }
  }

  async delete(key: string): Promise<void> {
    try {
      const db = await this.getDb();
      if (!db.objectStoreNames.contains(STORES.SASS_CACHE)) {
        return;
      }
      await db.delete(STORES.SASS_CACHE, key);
    } catch (error) {
      console.warn('SASS cache delete failed:', error);
    }
  }

  async getAllSortedByLastUsed(): Promise<SassCacheEntry[]> {
    try {
      const db = await this.getDb();
      if (!db.objectStoreNames.contains(STORES.SASS_CACHE)) {
        return [];
      }
      const tx = db.transaction(STORES.SASS_CACHE, 'readonly');
      const store = tx.objectStore(STORES.SASS_CACHE);
      const index = store.index('lastUsed');
      return (await index.getAll()) as SassCacheEntry[];
    } catch {
      return [];
    }
  }

  async clear(): Promise<void> {
    try {
      const db = await this.getDb();
      if (!db.objectStoreNames.contains(STORES.SASS_CACHE)) {
        return;
      }
      await db.clear(STORES.SASS_CACHE);
    } catch (error) {
      console.warn('SASS cache clear failed:', error);
    }
  }
}

// ============================================================================
// In-Memory Storage Backend (Testing)
// ============================================================================

/**
 * In-memory storage for the SASS cache.
 *
 * Used in unit tests to verify caching logic without IndexedDB.
 */
export class InMemoryCacheStorage implements SassCacheStorage {
  private entries: Map<string, SassCacheEntry> = new Map();

  async isAvailable(): Promise<boolean> {
    return true;
  }

  async get(key: string): Promise<SassCacheEntry | undefined> {
    return this.entries.get(key);
  }

  async put(entry: SassCacheEntry): Promise<void> {
    this.entries.set(entry.key, entry);
  }

  async delete(key: string): Promise<void> {
    this.entries.delete(key);
  }

  async getAllSortedByLastUsed(): Promise<SassCacheEntry[]> {
    const entries = Array.from(this.entries.values());
    // Sort by lastUsed ascending (oldest first)
    return entries.sort((a, b) => a.lastUsed - b.lastUsed);
  }

  async clear(): Promise<void> {
    this.entries.clear();
  }

  /** Get the number of entries (for testing) */
  get size(): number {
    return this.entries.size;
  }
}

// ============================================================================
// SASS Cache Manager
// ============================================================================

/**
 * SASS Cache Manager with LRU eviction.
 *
 * Caches compiled CSS to avoid recompilation.
 * Uses LRU (Least Recently Used) eviction when size or entry limits are exceeded.
 *
 * The storage backend is injectable for testability:
 * - Production: IndexedDBCacheStorage (persistent)
 * - Testing: InMemoryCacheStorage (in-memory)
 *
 * @example
 * ```typescript
 * // Production usage
 * const cache = new SassCacheManager();
 *
 * // Test usage with in-memory storage
 * const storage = new InMemoryCacheStorage();
 * const cache = new SassCacheManager({ storage });
 *
 * // Check cache before compilation
 * const cacheKey = await cache.computeKey(scss, minified);
 * const cached = await cache.get(cacheKey);
 * if (cached) {
 *   return cached;
 * }
 *
 * // Compile and cache
 * const css = await compileSass(scss, minified);
 * await cache.set(cacheKey, css, await computeHash(scss), minified);
 * return css;
 * ```
 */
export class SassCacheManager {
  private config: SassCacheConfig;
  private storage: SassCacheStorage;

  constructor(options: {
    config?: Partial<SassCacheConfig>;
    storage?: SassCacheStorage;
  } = {}) {
    this.config = { ...DEFAULT_CACHE_CONFIG, ...options.config };
    this.storage = options.storage ?? new IndexedDBCacheStorage();
  }

  /**
   * Get the current configuration.
   */
  getConfig(): SassCacheConfig {
    return { ...this.config };
  }

  /**
   * Compute a cache key from SCSS content and options.
   *
   * The key is a SHA-256 hash of the SCSS content concatenated with
   * the minified flag, ensuring different compilation options produce
   * different cache entries.
   */
  async computeKey(scss: string, minified: boolean): Promise<string> {
    const input = `${scss}:minified=${minified}`;
    return computeHash(input);
  }

  /**
   * Get a cached CSS entry by key.
   *
   * Updates the lastUsed timestamp if found (touch on read).
   *
   * @returns The cached CSS string, or null if not found
   */
  async get(key: string): Promise<string | null> {
    try {
      if (!(await this.storage.isAvailable())) {
        return null;
      }

      const entry = await this.storage.get(key);

      if (entry) {
        // Update lastUsed timestamp (touch on read for LRU)
        await this.touch(key);
        return entry.css;
      }

      return null;
    } catch (error) {
      console.warn('SASS cache get failed:', error);
      return null;
    }
  }

  /**
   * Store compiled CSS in the cache.
   *
   * Automatically prunes the cache if size or entry limits are exceeded.
   */
  async set(
    key: string,
    css: string,
    sourceHash: string,
    minified: boolean
  ): Promise<void> {
    try {
      if (!(await this.storage.isAvailable())) {
        return;
      }

      const now = Date.now();
      const size = computeSize(css);

      const entry: SassCacheEntry = {
        key,
        css,
        created: now,
        lastUsed: now,
        size,
        sourceHash,
        minified,
      };

      await this.storage.put(entry);

      // Prune cache if needed (async, don't block)
      this.prune().catch((err) => {
        console.warn('SASS cache prune failed:', err);
      });
    } catch (error) {
      console.warn('SASS cache set failed:', error);
    }
  }

  /**
   * Update the lastUsed timestamp for a cache entry.
   */
  async touch(key: string): Promise<void> {
    try {
      if (!(await this.storage.isAvailable())) {
        return;
      }

      const entry = await this.storage.get(key);

      if (entry) {
        entry.lastUsed = Date.now();
        await this.storage.put(entry);
      }
    } catch {
      // Silently ignore touch failures
    }
  }

  /**
   * Prune the cache to stay within size and entry limits.
   *
   * Uses LRU eviction - removes least recently used entries first.
   */
  async prune(): Promise<void> {
    if (!(await this.storage.isAvailable())) {
      return;
    }

    // Get all entries sorted by lastUsed (oldest first)
    const entries = await this.storage.getAllSortedByLastUsed();

    // Calculate current totals
    let totalSize = entries.reduce((sum, e) => sum + e.size, 0);
    let entryCount = entries.length;

    // Evict oldest entries until we're under limits
    // Entries are already sorted by lastUsed (oldest first)
    for (const entry of entries) {
      if (totalSize <= this.config.maxSizeBytes && entryCount <= this.config.maxEntries) {
        break;
      }

      await this.storage.delete(entry.key);
      totalSize -= entry.size;
      entryCount--;
    }
  }

  /**
   * Clear all entries from the cache.
   */
  async clear(): Promise<void> {
    try {
      await this.storage.clear();
    } catch (error) {
      console.warn('SASS cache clear failed:', error);
    }
  }

  /**
   * Get statistics about the cache.
   */
  async getStats(): Promise<SassCacheStats> {
    try {
      if (!(await this.storage.isAvailable())) {
        return {
          entryCount: 0,
          totalSizeBytes: 0,
          oldestEntry: null,
          newestEntry: null,
        };
      }

      const entries = await this.storage.getAllSortedByLastUsed();

      if (entries.length === 0) {
        return {
          entryCount: 0,
          totalSizeBytes: 0,
          oldestEntry: null,
          newestEntry: null,
        };
      }

      const totalSizeBytes = entries.reduce((sum, e) => sum + e.size, 0);
      const timestamps = entries.map((e) => e.lastUsed);

      return {
        entryCount: entries.length,
        totalSizeBytes,
        oldestEntry: Math.min(...timestamps),
        newestEntry: Math.max(...timestamps),
      };
    } catch (error) {
      console.warn('SASS cache getStats failed:', error);
      return {
        entryCount: 0,
        totalSizeBytes: 0,
        oldestEntry: null,
        newestEntry: null,
      };
    }
  }

  /**
   * Check if a key exists in the cache without updating lastUsed.
   */
  async has(key: string): Promise<boolean> {
    try {
      if (!(await this.storage.isAvailable())) {
        return false;
      }
      const entry = await this.storage.get(key);
      return entry !== undefined;
    } catch {
      return false;
    }
  }

  /**
   * Delete a specific entry from the cache.
   */
  async delete(key: string): Promise<boolean> {
    try {
      if (!(await this.storage.isAvailable())) {
        return false;
      }

      const exists = await this.has(key);
      if (exists) {
        await this.storage.delete(key);
        return true;
      }
      return false;
    } catch {
      return false;
    }
  }
}

// ============================================================================
// Utility Functions
// ============================================================================

/**
 * Compute the size of a string in bytes.
 *
 * Uses TextEncoder for accurate UTF-8 byte count.
 */
export function computeSize(str: string): number {
  return new TextEncoder().encode(str).length;
}

/**
 * Compute a SHA-256 hash of a string.
 *
 * Uses the Web Crypto API for secure hashing.
 */
export async function computeHash(input: string): Promise<string> {
  const encoder = new TextEncoder();
  const data = encoder.encode(input);
  const hashBuffer = await crypto.subtle.digest('SHA-256', data);
  const hashArray = Array.from(new Uint8Array(hashBuffer));
  return hashArray.map((b) => b.toString(16).padStart(2, '0')).join('');
}

/**
 * Simple hash function for environments without crypto.subtle (e.g., Node.js tests).
 *
 * NOT cryptographically secure - only use for testing.
 */
export function computeHashSync(input: string): string {
  let hash = 0;
  for (let i = 0; i < input.length; i++) {
    const char = input.charCodeAt(i);
    hash = ((hash << 5) - hash) + char;
    hash = hash & hash; // Convert to 32-bit integer
  }
  return Math.abs(hash).toString(16).padStart(8, '0');
}

// ============================================================================
// Singleton / Factory
// ============================================================================

/**
 * Singleton instance of the SASS cache manager.
 *
 * Use this for the default cache configuration.
 */
let defaultCacheInstance: SassCacheManager | null = null;

/**
 * Get the default SASS cache manager instance.
 */
export function getSassCache(): SassCacheManager {
  if (!defaultCacheInstance) {
    defaultCacheInstance = new SassCacheManager();
  }
  return defaultCacheInstance;
}

/**
 * Reset the default cache instance (for testing).
 */
export function resetSassCache(): void {
  defaultCacheInstance = null;
}
