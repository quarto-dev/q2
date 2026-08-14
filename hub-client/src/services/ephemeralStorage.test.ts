/**
 * Tests for ephemeral storage mode (bd-sw4xy1vw).
 *
 * The q2 preview embed build sets VITE_EPHEMERAL_STORAGE=1: the app must
 * then keep every automerge document and hub record in memory instead of
 * IndexedDB, because each preview session is a fresh origin and nothing
 * persisted is ever read again.
 */

import { describe, it, expect, afterEach, vi } from 'vitest';
import { MemoryStorageAdapter } from '@quarto/quarto-sync-client';

// IndexedDBStorageAdapter's constructor reads the indexedDB global eagerly
// and throws in node; mock the class so adapter selection can be observed
// either way.
vi.mock('@automerge/automerge-repo-storage-indexeddb', () => ({
  IndexedDBStorageAdapter: class IndexedDBStorageAdapter {},
}));

import { IndexedDBStorageAdapter } from '@automerge/automerge-repo-storage-indexeddb';
import { isEphemeralStorage, repoStorageAdapter } from './ephemeralStorage';

afterEach(() => {
  vi.unstubAllEnvs();
});

describe('isEphemeralStorage', () => {
  it('is false by default', () => {
    expect(isEphemeralStorage()).toBe(false);
  });

  it('is true when VITE_EPHEMERAL_STORAGE=1', () => {
    vi.stubEnv('VITE_EPHEMERAL_STORAGE', '1');
    expect(isEphemeralStorage()).toBe(true);
  });

  it('is false for other values', () => {
    vi.stubEnv('VITE_EPHEMERAL_STORAGE', '0');
    expect(isEphemeralStorage()).toBe(false);
    vi.stubEnv('VITE_EPHEMERAL_STORAGE', 'true');
    expect(isEphemeralStorage()).toBe(false);
  });
});

describe('repoStorageAdapter', () => {
  it('returns the IndexedDB adapter by default', () => {
    expect(repoStorageAdapter()).toBeInstanceOf(IndexedDBStorageAdapter);
  });

  it('returns the in-memory adapter when ephemeral', () => {
    vi.stubEnv('VITE_EPHEMERAL_STORAGE', '1');
    expect(repoStorageAdapter()).toBeInstanceOf(MemoryStorageAdapter);
  });
});
