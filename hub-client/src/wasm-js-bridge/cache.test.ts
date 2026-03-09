import "fake-indexeddb/auto";
import { describe, it, expect, beforeEach } from "vitest";
import {
  jsCacheGet,
  jsCacheSet,
  jsCacheDelete,
  jsCacheClearNamespace,
  _resetDbHandle,
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
});
