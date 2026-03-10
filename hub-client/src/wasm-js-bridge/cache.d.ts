/**
 * Type declarations for the cache bridge module.
 */

/** Maximum number of entries before eviction triggers. */
export const MAX_ENTRIES: number;

/** Maximum total size in bytes before eviction triggers (50MB). */
export const MAX_TOTAL_SIZE: number;

/**
 * Get a cached value by namespace and key.
 *
 * On a cache hit, the entry's timestamp is updated (touch-on-read)
 * so that actively-used entries survive LRU eviction.
 *
 * @param namespace - Cache namespace (e.g. "sass", "metadata")
 * @param key - Cache key (typically a hex-encoded hash)
 * @returns The cached bytes, or null on miss
 */
export function jsCacheGet(
  namespace: string,
  key: string
): Promise<Uint8Array | null>;

/**
 * Store a value in the cache. Evicts oldest entries if limits are exceeded.
 *
 * @param namespace - Cache namespace
 * @param key - Cache key
 * @param value - The bytes to cache
 */
export function jsCacheSet(
  namespace: string,
  key: string,
  value: Uint8Array
): Promise<void>;

/**
 * Delete a cached value by namespace and key.
 *
 * @param namespace - Cache namespace
 * @param key - Cache key
 */
export function jsCacheDelete(namespace: string, key: string): Promise<void>;

/**
 * Clear all cached values in a namespace.
 *
 * @param namespace - Cache namespace to clear
 */
export function jsCacheClearNamespace(namespace: string): Promise<void>;

/**
 * Reset the module-level database handle (for testing only).
 */
export function _resetDbHandle(): void;
