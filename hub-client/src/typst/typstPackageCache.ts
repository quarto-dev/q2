import type { TarballCache } from '@quarto/typst-host';

export const TYPST_PACKAGE_CACHE = 'q2-typst-packages-v1';

/**
 * The Cache API as the typst package tarball cache (design T3): tarballs are immutable per
 * version, so an entry never needs revalidation. Every failure is swallowed by the caller
 * (`registryFetcher` treats the cache as best-effort), and an unavailable Cache API is a no-op.
 */
export function cacheApiTarballs(caches: CacheStorage | undefined = typeof globalThis.caches === 'undefined' ? undefined : globalThis.caches): TarballCache | undefined {
  if (!caches) return undefined;
  return {
    async get(url) {
      const cache = await caches.open(TYPST_PACKAGE_CACHE);
      const hit = await cache.match(url);
      return hit ? new Uint8Array(await hit.arrayBuffer()) : undefined;
    },
    async put(url, bytes) {
      const cache = await caches.open(TYPST_PACKAGE_CACHE);
      await cache.put(url, new Response(bytes as BodyInit, { headers: { 'content-type': 'application/gzip' } }));
    },
  };
}
