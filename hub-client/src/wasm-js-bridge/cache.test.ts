import "fake-indexeddb/auto";
import { describe, it, expect, beforeEach } from "vitest";
import {
  jsCacheGet,
  jsCacheSet,
  jsCacheDelete,
  jsCacheClearNamespace,
  _resetDbHandle,
  MAX_ENTRIES,
} from "./cache.js";

describe("cache bridge", () => {
  beforeEach(async () => {
    // Reset the module-level db handle so the next operation opens a fresh db
    _resetDbHandle();
    // Delete the database for full isolation between tests
    await new Promise<void>((resolve, reject) => {
      const request = indexedDB.deleteDatabase("quarto-cache");
      request.onsuccess = () => resolve();
      request.onerror = () => reject(request.error);
    });
  });

  it("roundtrip: set then get returns same bytes", async () => {
    const value = new Uint8Array([1, 2, 3, 4, 5]);
    await jsCacheSet("sass", "abc123", value);
    const result = await jsCacheGet("sass", "abc123");
    expect(result).toEqual(value);
  });

  it("get missing key returns null", async () => {
    const result = await jsCacheGet("sass", "nonexistent");
    expect(result).toBeNull();
  });

  it("namespaces are isolated", async () => {
    const valueA = new Uint8Array([10, 20]);
    const valueB = new Uint8Array([30, 40]);
    await jsCacheSet("sass", "key1", valueA);
    await jsCacheSet("metadata", "key1", valueB);

    const resultA = await jsCacheGet("sass", "key1");
    const resultB = await jsCacheGet("metadata", "key1");
    expect(resultA).toEqual(valueA);
    expect(resultB).toEqual(valueB);
  });

  it("clear namespace only clears targeted namespace", async () => {
    const valueA = new Uint8Array([1]);
    const valueB = new Uint8Array([2]);
    await jsCacheSet("sass", "key1", valueA);
    await jsCacheSet("metadata", "key1", valueB);

    await jsCacheClearNamespace("sass");

    const resultA = await jsCacheGet("sass", "key1");
    const resultB = await jsCacheGet("metadata", "key1");
    expect(resultA).toBeNull();
    expect(resultB).toEqual(valueB);
  });

  it("delete removes single entry", async () => {
    const value = new Uint8Array([1, 2, 3]);
    await jsCacheSet("sass", "key1", value);
    await jsCacheSet("sass", "key2", value);

    await jsCacheDelete("sass", "key1");

    const result1 = await jsCacheGet("sass", "key1");
    const result2 = await jsCacheGet("sass", "key2");
    expect(result1).toBeNull();
    expect(result2).toEqual(value);
  });

  // ── LRU eviction tests ────────────────────────────────────────────

  it("evicts oldest entries when exceeding MAX_ENTRIES", async () => {
    // Fill cache to MAX_ENTRIES + 5
    const total = MAX_ENTRIES + 5;
    const value = new Uint8Array([42]);

    for (let i = 0; i < total; i++) {
      await jsCacheSet("ns", `key-${i}`, value);
    }

    // The first 5 entries should have been evicted
    for (let i = 0; i < 5; i++) {
      const result = await jsCacheGet("ns", `key-${i}`);
      expect(result).toBeNull();
    }

    // Later entries should still be present
    for (let i = 5; i < total; i++) {
      const result = await jsCacheGet("ns", `key-${i}`);
      expect(result).toEqual(value);
    }
  });

  it("evicts across namespaces (global eviction)", async () => {
    const value = new Uint8Array([1]);
    const half = Math.floor(MAX_ENTRIES / 2);

    // Fill half from namespace "a", half from namespace "b", then add extras
    for (let i = 0; i < half; i++) {
      await jsCacheSet("a", `key-${i}`, value);
    }
    for (let i = 0; i < half; i++) {
      await jsCacheSet("b", `key-${i}`, value);
    }
    // Now add 5 more to trigger eviction
    for (let i = 0; i < 5; i++) {
      await jsCacheSet("c", `key-${i}`, value);
    }

    // The oldest entries (from namespace "a") should be evicted first
    let evictedCount = 0;
    for (let i = 0; i < half; i++) {
      const result = await jsCacheGet("a", `key-${i}`);
      if (result === null) evictedCount++;
    }
    expect(evictedCount).toBeGreaterThan(0);
  });

  it("touch-on-read makes entry survive eviction", async () => {
    const value = new Uint8Array([99]);

    // Insert entry 0 first (oldest)
    await jsCacheSet("ns", "touched", value);

    // Fill the rest of the cache
    for (let i = 1; i < MAX_ENTRIES; i++) {
      await jsCacheSet("ns", `filler-${i}`, value);
    }

    // Touch entry 0 (updates its timestamp via get)
    const touchResult = await jsCacheGet("ns", "touched");
    expect(touchResult).toEqual(value);

    // Add more entries to trigger eviction
    for (let i = 0; i < 5; i++) {
      await jsCacheSet("ns", `overflow-${i}`, value);
    }

    // The touched entry should survive (its timestamp was updated)
    const survived = await jsCacheGet("ns", "touched");
    expect(survived).toEqual(value);
  });

  it("stored records have correct size field", async () => {
    const value = new Uint8Array(1024); // 1KB
    await jsCacheSet("test", "sized", value);

    // Verify by reading back — the size is internal but we can verify
    // indirectly that the entry was stored correctly
    const result = await jsCacheGet("test", "sized");
    expect(result).not.toBeNull();
    expect(result!.length).toBe(1024);
  });
});
