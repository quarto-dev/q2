/**
 * `requireOnline` semantics (bd-xnmd5ni1): server-backed callers (the
 * hub MCP server) must be able to demand a live peer connection
 * instead of the silent offline fallback. With in-memory storage,
 * "offline mode" persists nothing — succeeding silently is a data
 * black hole: `create_project` reported an indexDocId for documents
 * that existed only in process memory and died with it.
 *
 * Defaults stay offline-first (hub-client's IndexedDB-backed browser
 * behavior is correct and untouched); `requireOnline: true` turns the
 * peer-wait timeout into a typed PeerUnavailableError.
 *
 * No mocks: the "server" is a port nothing listens on, so the peer
 * wait genuinely times out.
 */

import { describe, it, expect } from 'vitest';

import { createSyncClient, PeerUnavailableError } from './client.js';

// Port 9 (discard) on localhost: connection refused immediately.
const UNREACHABLE = 'ws://127.0.0.1:9/ws';

function client() {
  return createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onFileRemoved: () => {},
  });
}

describe('createNewProject + requireOnline', () => {
  it('rejects with PeerUnavailableError when the peer cannot be reached', async () => {
    await expect(
      client().createNewProject({
        syncServer: UNREACHABLE,
        files: [{ path: 'a.qmd', content: 'x', contentType: 'text' }],
        storage: 'memory',
        peerTimeoutMs: 150,
        requireOnline: true,
      }),
    ).rejects.toBeInstanceOf(PeerUnavailableError);
  });

  it('error names the server and the timeout', async () => {
    const err = await client()
      .createNewProject({
        syncServer: UNREACHABLE,
        files: [],
        storage: 'memory',
        peerTimeoutMs: 150,
        requireOnline: true,
      })
      .catch((e: unknown) => e);
    expect(String(err)).toContain('127.0.0.1:9');
    expect(String(err)).toContain('150');
  });

  it('default (no requireOnline) preserves the offline fallback', async () => {
    // Locks the browser-path contract: offline creation still works.
    const result = await client().createNewProject({
      syncServer: UNREACHABLE,
      files: [{ path: 'a.qmd', content: 'x', contentType: 'text' }],
      storage: 'memory',
      peerTimeoutMs: 50,
    });
    expect(result.indexDocId).toBeTruthy();
  });
});

describe('connect + requireOnline', () => {
  it('rejects with PeerUnavailableError when the peer cannot be reached', async () => {
    await expect(
      client().connect(UNREACHABLE, 'badc0ffee0ddf00d', undefined, undefined, undefined, {
        storage: 'memory',
        peerTimeoutMs: 150,
        requireOnline: true,
      }),
    ).rejects.toBeInstanceOf(PeerUnavailableError);
  });
});
