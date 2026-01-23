/**
 * Tests for SASS Cache Manager.
 *
 * These tests verify the caching logic including:
 * - Cache hit/miss scenarios
 * - LRU eviction based on entry count
 * - LRU eviction based on size limits
 * - Touch-on-read behavior
 * - Cache statistics
 */

import { describe, it, expect, beforeEach } from 'vitest';
import {
  SassCacheManager,
  InMemoryCacheStorage,
  computeSize,
  computeHashSync,
} from './sassCache';

describe('SassCacheManager', () => {
  let storage: InMemoryCacheStorage;
  let cache: SassCacheManager;

  beforeEach(() => {
    storage = new InMemoryCacheStorage();
    cache = new SassCacheManager({
      storage,
      config: {
        maxEntries: 5,
        maxSizeBytes: 1000,
      },
    });
  });

  describe('basic operations', () => {
    it('returns null for cache miss', async () => {
      const result = await cache.get('nonexistent-key');
      expect(result).toBeNull();
    });

    it('stores and retrieves cached CSS', async () => {
      const key = 'test-key';
      const css = '.test { color: blue; }';

      await cache.set(key, css, 'source-hash', false);
      const result = await cache.get(key);

      expect(result).toBe(css);
    });

    it('returns null after cache is cleared', async () => {
      const key = 'test-key';
      const css = '.test { color: blue; }';

      await cache.set(key, css, 'source-hash', false);
      await cache.clear();
      const result = await cache.get(key);

      expect(result).toBeNull();
    });

    it('deletes specific entries', async () => {
      await cache.set('key1', 'css1', 'hash1', false);
      await cache.set('key2', 'css2', 'hash2', false);

      const deleted = await cache.delete('key1');
      expect(deleted).toBe(true);

      expect(await cache.get('key1')).toBeNull();
      expect(await cache.get('key2')).toBe('css2');
    });

    it('returns false when deleting nonexistent key', async () => {
      const deleted = await cache.delete('nonexistent');
      expect(deleted).toBe(false);
    });

    it('has() checks existence without touching', async () => {
      await cache.set('key1', 'css1', 'hash1', false);

      // Get the initial lastUsed
      const entriesBefore = await storage.getAllSortedByLastUsed();
      const lastUsedBefore = entriesBefore[0].lastUsed;

      // Wait a bit then check with has()
      await new Promise(resolve => setTimeout(resolve, 10));
      const exists = await cache.has('key1');

      expect(exists).toBe(true);

      // lastUsed should not have changed
      const entriesAfter = await storage.getAllSortedByLastUsed();
      expect(entriesAfter[0].lastUsed).toBe(lastUsedBefore);
    });
  });

  describe('touch on read', () => {
    it('updates lastUsed timestamp when getting an entry', async () => {
      await cache.set('key1', 'css1', 'hash1', false);

      // Get the initial lastUsed
      const entriesBefore = await storage.getAllSortedByLastUsed();
      const lastUsedBefore = entriesBefore[0].lastUsed;

      // Wait a bit then get the entry
      await new Promise(resolve => setTimeout(resolve, 10));
      await cache.get('key1');

      // lastUsed should have changed
      const entriesAfter = await storage.getAllSortedByLastUsed();
      expect(entriesAfter[0].lastUsed).toBeGreaterThan(lastUsedBefore);
    });
  });

  describe('LRU eviction by entry count', () => {
    it('evicts oldest entries when exceeding maxEntries', async () => {
      // Add 5 entries (at limit)
      for (let i = 0; i < 5; i++) {
        await cache.set(`key${i}`, `css${i}`, `hash${i}`, false);
        // Small delay to ensure different timestamps
        await new Promise(resolve => setTimeout(resolve, 5));
      }

      // Verify all 5 exist
      expect(storage.size).toBe(5);

      // Add one more (exceeds limit)
      await cache.set('key5', 'css5', 'hash5', false);

      // Wait for prune to complete
      await new Promise(resolve => setTimeout(resolve, 50));

      // Should have evicted oldest entry
      expect(storage.size).toBeLessThanOrEqual(5);
      expect(await cache.get('key0')).toBeNull(); // Oldest should be gone
      expect(await cache.get('key5')).toBe('css5'); // Newest should exist
    });

    it('keeps recently accessed entries during eviction', async () => {
      // Add 5 entries
      for (let i = 0; i < 5; i++) {
        await cache.set(`key${i}`, `css${i}`, `hash${i}`, false);
        await new Promise(resolve => setTimeout(resolve, 5));
      }

      // Access key0 to make it recently used
      await cache.get('key0');
      await new Promise(resolve => setTimeout(resolve, 5));

      // Add more entries to trigger eviction
      await cache.set('key5', 'css5', 'hash5', false);
      await new Promise(resolve => setTimeout(resolve, 50));

      // key0 should still exist (was recently accessed)
      // key1 should be evicted (oldest non-accessed)
      expect(await cache.has('key0')).toBe(true);
    });
  });

  describe('LRU eviction by size', () => {
    it('evicts entries when exceeding maxSizeBytes', async () => {
      // Create cache with small size limit
      const smallCache = new SassCacheManager({
        storage,
        config: {
          maxEntries: 100,
          maxSizeBytes: 100, // 100 bytes
        },
      });

      // Add entries that together exceed 100 bytes
      // Each "cssXXX" string is about 6 bytes
      for (let i = 0; i < 10; i++) {
        await smallCache.set(`key${i}`, `css${i.toString().padStart(10, '0')}`, `hash${i}`, false);
        await new Promise(resolve => setTimeout(resolve, 5));
      }

      // Wait for prune to complete
      await new Promise(resolve => setTimeout(resolve, 50));

      // Should have evicted some entries to stay under 100 bytes
      const stats = await smallCache.getStats();
      expect(stats.totalSizeBytes).toBeLessThanOrEqual(100);
    });
  });

  describe('cache statistics', () => {
    it('returns correct stats for empty cache', async () => {
      const stats = await cache.getStats();

      expect(stats.entryCount).toBe(0);
      expect(stats.totalSizeBytes).toBe(0);
      expect(stats.oldestEntry).toBeNull();
      expect(stats.newestEntry).toBeNull();
    });

    it('returns correct stats after adding entries', async () => {
      const css1 = '.test1 { color: blue; }';
      const css2 = '.test2 { color: red; }';

      await cache.set('key1', css1, 'hash1', false);
      await new Promise(resolve => setTimeout(resolve, 10));
      await cache.set('key2', css2, 'hash2', false);

      const stats = await cache.getStats();

      expect(stats.entryCount).toBe(2);
      expect(stats.totalSizeBytes).toBe(computeSize(css1) + computeSize(css2));
      expect(stats.oldestEntry).not.toBeNull();
      expect(stats.newestEntry).not.toBeNull();
      expect(stats.newestEntry).toBeGreaterThan(stats.oldestEntry!);
    });
  });

  describe('cache key computation', () => {
    it('produces different keys for different content', async () => {
      const key1 = computeHashSync('content1:minified=false');
      const key2 = computeHashSync('content2:minified=false');

      expect(key1).not.toBe(key2);
    });

    it('produces different keys for different minified options', async () => {
      const key1 = computeHashSync('content:minified=false');
      const key2 = computeHashSync('content:minified=true');

      expect(key1).not.toBe(key2);
    });

    it('produces same key for same input', async () => {
      const key1 = computeHashSync('content:minified=false');
      const key2 = computeHashSync('content:minified=false');

      expect(key1).toBe(key2);
    });
  });

  describe('configuration', () => {
    it('uses provided configuration', () => {
      const customCache = new SassCacheManager({
        config: {
          maxSizeBytes: 1000000,
          maxEntries: 500,
        },
      });

      const config = customCache.getConfig();
      expect(config.maxSizeBytes).toBe(1000000);
      expect(config.maxEntries).toBe(500);
    });

    it('uses defaults for missing config values', () => {
      const defaultCache = new SassCacheManager({});
      const config = defaultCache.getConfig();

      expect(config.maxSizeBytes).toBe(50 * 1024 * 1024);
      expect(config.maxEntries).toBe(1000);
    });
  });
});

describe('InMemoryCacheStorage', () => {
  it('is always available', async () => {
    const storage = new InMemoryCacheStorage();
    expect(await storage.isAvailable()).toBe(true);
  });

  it('stores and retrieves entries', async () => {
    const storage = new InMemoryCacheStorage();
    const entry = {
      key: 'test',
      css: '.test { }',
      created: Date.now(),
      lastUsed: Date.now(),
      size: 10,
      sourceHash: 'hash',
      minified: false,
    };

    await storage.put(entry);
    const retrieved = await storage.get('test');

    expect(retrieved).toEqual(entry);
  });

  it('returns entries sorted by lastUsed', async () => {
    const storage = new InMemoryCacheStorage();

    const now = Date.now();
    await storage.put({ key: 'c', css: '', created: now, lastUsed: now + 200, size: 0, sourceHash: '', minified: false });
    await storage.put({ key: 'a', css: '', created: now, lastUsed: now, size: 0, sourceHash: '', minified: false });
    await storage.put({ key: 'b', css: '', created: now, lastUsed: now + 100, size: 0, sourceHash: '', minified: false });

    const sorted = await storage.getAllSortedByLastUsed();

    expect(sorted[0].key).toBe('a');
    expect(sorted[1].key).toBe('b');
    expect(sorted[2].key).toBe('c');
  });
});

describe('computeSize', () => {
  it('computes ASCII string size correctly', () => {
    expect(computeSize('hello')).toBe(5);
    expect(computeSize('')).toBe(0);
    expect(computeSize('a')).toBe(1);
  });

  it('computes multi-byte character size correctly', () => {
    // UTF-8: each emoji is 4 bytes
    expect(computeSize('\u{1F600}')).toBe(4); // grinning face emoji
    // Mix of ASCII and multi-byte
    expect(computeSize('hello\u{1F600}')).toBe(9); // 5 + 4
  });
});
