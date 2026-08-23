/**
 * Tests for the ephemeral in-memory cache mode (bd-91mdd056).
 *
 * When the host page sets `globalThis.__Q2_EPHEMERAL_STORAGE__` before
 * the WASM first touches the cache, the bridge keeps everything in a
 * module-level Map instead of IndexedDB. q2 preview sessions need this:
 * each session is a fresh origin (a random loopback port), so a
 * persisted `quarto-cache` database is never read again and just
 * accumulates.
 *
 * This file deliberately does NOT import fake-indexeddb: the node
 * environment has no `indexedDB` global, so any accidental fall-through
 * to the persistent path throws instead of silently passing.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  jsCacheGet,
  jsCacheSet,
  jsCacheDelete,
  jsCacheClearNamespace,
  _resetDbHandle,
  MAX_ENTRIES,
} from "./cache.js";

describe("cache bridge (ephemeral in-memory mode)", () => {
  beforeEach(() => {
    (globalThis as Record<string, unknown>).__Q2_EPHEMERAL_STORAGE__ = true;
    _resetDbHandle();
    // Guard: if fake-indexeddb (or a real DOM) is ever present here, the
    // "never touches IndexedDB" property is unproven.
    expect(globalThis.indexedDB).toBeUndefined();
  });

  afterEach(() => {
    delete (globalThis as Record<string, unknown>).__Q2_EPHEMERAL_STORAGE__;
    _resetDbHandle();
  });

  it("roundtrip: set then get returns same bytes", async () => {
    const value = new Uint8Array([1, 2, 3, 4, 5]);
    await jsCacheSet("sass", "abc123", value);
    expect(await jsCacheGet("sass", "abc123")).toEqual(value);
  });

  it("get missing key returns null", async () => {
    expect(await jsCacheGet("sass", "nonexistent")).toBeNull();
  });

  it("namespaces are isolated", async () => {
    const valueA = new Uint8Array([10, 20]);
    const valueB = new Uint8Array([30, 40]);
    await jsCacheSet("sass", "key1", valueA);
    await jsCacheSet("metadata", "key1", valueB);

    expect(await jsCacheGet("sass", "key1")).toEqual(valueA);
    expect(await jsCacheGet("metadata", "key1")).toEqual(valueB);
  });

  it("clear namespace only clears targeted namespace", async () => {
    await jsCacheSet("sass", "key1", new Uint8Array([1]));
    await jsCacheSet("metadata", "key1", new Uint8Array([2]));

    await jsCacheClearNamespace("sass");

    expect(await jsCacheGet("sass", "key1")).toBeNull();
    expect(await jsCacheGet("metadata", "key1")).toEqual(new Uint8Array([2]));
  });

  it("delete removes single entry", async () => {
    const value = new Uint8Array([1, 2, 3]);
    await jsCacheSet("sass", "key1", value);
    await jsCacheSet("sass", "key2", value);

    await jsCacheDelete("sass", "key1");

    expect(await jsCacheGet("sass", "key1")).toBeNull();
    expect(await jsCacheGet("sass", "key2")).toEqual(value);
  });

  it("evicts oldest entries when exceeding MAX_ENTRIES", async () => {
    const total = MAX_ENTRIES + 5;
    const value = new Uint8Array([42]);

    for (let i = 0; i < total; i++) {
      await jsCacheSet("ns", `key-${i}`, value);
    }

    for (let i = 0; i < 5; i++) {
      expect(await jsCacheGet("ns", `key-${i}`)).toBeNull();
    }
    for (let i = 5; i < total; i++) {
      expect(await jsCacheGet("ns", `key-${i}`)).toEqual(value);
    }
  });

  it("touch-on-read makes entry survive eviction", async () => {
    const value = new Uint8Array([99]);

    await jsCacheSet("ns", "touched", value);
    for (let i = 1; i < MAX_ENTRIES; i++) {
      await jsCacheSet("ns", `filler-${i}`, value);
    }

    // Touch the oldest entry, then overflow the cache.
    expect(await jsCacheGet("ns", "touched")).toEqual(value);
    for (let i = 0; i < 5; i++) {
      await jsCacheSet("ns", `overflow-${i}`, value);
    }

    expect(await jsCacheGet("ns", "touched")).toEqual(value);
  });

  // The test above only fails when the whole body happens to land inside
  // one Date.now() millisecond, which made it flaky rather than wrong
  // (GH #250 surfaced it on a macOS runner). Pinning the clock forces
  // that worst case every run: with every timestamp equal, the eviction
  // order is decided purely by how jsCacheGet records a touch, so this
  // binds the fix rather than the timing.
  it("touch-on-read survives eviction when every timestamp ties", async () => {
    const clock = vi.spyOn(Date, "now").mockReturnValue(1_000_000);
    try {
      const value = new Uint8Array([99]);

      await jsCacheSet("ns", "touched", value);
      for (let i = 1; i < MAX_ENTRIES; i++) {
        await jsCacheSet("ns", `filler-${i}`, value);
      }

      expect(await jsCacheGet("ns", "touched")).toEqual(value);
      for (let i = 0; i < 5; i++) {
        await jsCacheSet("ns", `overflow-${i}`, value);
      }

      // "touched" was inserted first, so a stable sort over tied
      // timestamps evicts it first unless the touch re-ordered it.
      expect(await jsCacheGet("ns", "touched")).toEqual(value);
      // And the untouched oldest filler is the one that should be gone.
      expect(await jsCacheGet("ns", "filler-1")).toBeNull();
    } finally {
      clock.mockRestore();
    }
  });

  it("does not persist across _resetDbHandle (the session boundary)", async () => {
    await jsCacheSet("sass", "abc123", new Uint8Array([1, 2, 3]));
    _resetDbHandle();
    expect(await jsCacheGet("sass", "abc123")).toBeNull();
  });

  it("persistent path rejects when the flag is unset and indexedDB is absent", async () => {
    // Proves the seam: with the flag off this environment cannot serve
    // the cache at all — the memory path is opt-in, not a silent
    // fallback that could mask a missing flag in a real session.
    delete (globalThis as Record<string, unknown>).__Q2_EPHEMERAL_STORAGE__;
    await expect(jsCacheSet("sass", "k", new Uint8Array([1]))).rejects.toThrow();
  });
});
