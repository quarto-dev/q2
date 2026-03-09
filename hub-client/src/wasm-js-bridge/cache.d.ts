/**
 * Type declarations for the cache bridge module.
 */

/**
 * Get a cached value by namespace and key.
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
 * Store a value in the cache.
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
