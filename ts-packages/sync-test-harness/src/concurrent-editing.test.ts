/**
 * Concurrent editing integration tests.
 *
 * Two real sync clients connected to the same hub server edit the same
 * document concurrently. Verifies that:
 *
 * 1. applyEditorOperations (splice) preserves both peers' edits
 * 2. updateFileContent with stale state destroys the remote peer's edit
 *
 * This is the end-to-end regression test for the splice-based sync fix.
 */

// Polyfill IndexedDB for Node.js (required by automerge-repo-storage-indexeddb)
import 'fake-indexeddb/auto';

import { describe, test, beforeAll, afterAll, expect } from 'vitest';
import {
  createSyncClient,
  type SyncClientCallbacks,
  type FilePayload,
  type Patch,
} from '@quarto/quarto-sync-client';
import { startHubServer, type ServerHandle } from './server-manager.js';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

interface TrackedClient {
  client: ReturnType<typeof createSyncClient>;
  files: Map<string, string>;
  errors: Error[];
  /** Resolves when onFileAdded or onFileChanged delivers content for `path`. */
  waitForFile(path: string, timeoutMs?: number): Promise<string>;
  /** Resolves when onFileChanged fires for `path` (ignores onFileAdded). */
  waitForChange(path: string, timeoutMs?: number): Promise<string>;
  /** Poll getFileContent until `predicate` passes or timeout. */
  waitForContent(path: string, predicate: (text: string) => boolean, timeoutMs?: number): Promise<string>;
}

function createTrackedClient(): TrackedClient {
  const files = new Map<string, string>();
  const errors: Error[] = [];
  const fileWaiters = new Map<string, Array<(text: string) => void>>();
  const changeWaiters = new Map<string, Array<(text: string) => void>>();

  function notifyWaiters(map: Map<string, Array<(text: string) => void>>, path: string, text: string) {
    const waiters = map.get(path);
    if (waiters) {
      for (const resolve of waiters) resolve(text);
      map.delete(path);
    }
  }

  const callbacks: SyncClientCallbacks = {
    onFileAdded(path: string, file: FilePayload) {
      if (file.type === 'text') {
        files.set(path, file.text);
        notifyWaiters(fileWaiters, path, file.text);
      }
    },
    onFileChanged(path: string, text: string, _patches: Patch[]) {
      files.set(path, text);
      notifyWaiters(fileWaiters, path, text);
      notifyWaiters(changeWaiters, path, text);
    },
    onBinaryChanged() {},
    onFileRemoved(path: string) {
      files.delete(path);
    },
    onFilesChange() {},
    onConnectionChange() {},
    onError(error: Error) {
      errors.push(error);
    },
  };

  const client = createSyncClient(callbacks);

  function makeWaiter(map: Map<string, Array<(text: string) => void>>, path: string, timeoutMs: number): Promise<string> {
    // If already available (for fileWaiters), resolve immediately
    if (map === fileWaiters && files.has(path)) {
      return Promise.resolve(files.get(path)!);
    }
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(
        () => reject(new Error(`Timed out waiting for ${path}`)),
        timeoutMs,
      );
      const waiters = map.get(path) ?? [];
      waiters.push((text) => {
        clearTimeout(timeout);
        resolve(text);
      });
      map.set(path, waiters);
    });
  }

  function waitForContent(path: string, predicate: (text: string) => boolean, timeoutMs = 5000): Promise<string> {
    // Check immediately
    const current = client.getFileContent(path);
    if (current !== null && predicate(current)) return Promise.resolve(current);

    return new Promise((resolve, reject) => {
      const deadline = Date.now() + timeoutMs;
      const interval = setInterval(() => {
        const text = client.getFileContent(path);
        if (text !== null && predicate(text)) {
          clearInterval(interval);
          resolve(text);
        } else if (Date.now() > deadline) {
          clearInterval(interval);
          reject(new Error(`Timed out waiting for content predicate on ${path} (last: ${text})`));
        }
      }, 50);
    });
  }

  return {
    client,
    files,
    errors,
    waitForFile: (path, timeoutMs = 5000) => makeWaiter(fileWaiters, path, timeoutMs),
    waitForChange: (path, timeoutMs = 5000) => makeWaiter(changeWaiters, path, timeoutMs),
    waitForContent,
  };
}

// ---------------------------------------------------------------------------
// Test suites — separated so the "bug demonstration" suite (updateFileContent
// only) is fully independent from the "fix verification" suite
// (applyEditorOperations). This avoids IndexedDB cross-contamination when one
// suite fails because the fix is missing.
// ---------------------------------------------------------------------------

const HUB_PORT = 18_300;
let server: ServerHandle;

beforeAll(async () => {
  server = await startHubServer({ port: HUB_PORT });
}, 120_000);

afterAll(async () => {
  await server?.stop();
});

// =========================================================================
// Suite 1: Demonstrate the updateFileContent race condition (the bug).
// Uses only the old API — passes with or without the splice fix.
// =========================================================================

describe('updateFileContent race condition', () => {
  test('stale content destroys remote edit', async () => {
    const clientA = createTrackedClient();
    const result = await clientA.client.createNewProject({
      syncServer: server.url,
      files: [{ path: 'doc.qmd', content: 'Hello world', contentType: 'text' }],
    });

    const clientB = createTrackedClient();
    await clientB.client.connect(server.url, result.indexDocId);
    await clientB.waitForFile('doc.qmd');

    // Client A inserts "REMOTE " at the start
    const changePromise = clientB.waitForChange('doc.qmd');
    clientA.client.updateFileContent('doc.qmd', 'REMOTE Hello world');
    await changePromise;

    expect(clientB.files.get('doc.qmd')).toBe('REMOTE Hello world');

    // Client B's editor was showing stale "Hello world" and user typed "!".
    // With updateFileContent, it sends the full stale text + keystroke:
    const changePromiseA = clientA.waitForChange('doc.qmd');
    clientB.client.updateFileContent('doc.qmd', 'Hello world!');
    await changePromiseA;

    // Wait for convergence
    await clientA.waitForContent('doc.qmd', (t) => !t.includes('REMOTE'));

    const textA = clientA.client.getFileContent('doc.qmd');
    const textB = clientB.client.getFileContent('doc.qmd');

    // updateText diffed "REMOTE Hello world" against "Hello world!" and
    // decided "REMOTE " should be deleted. This is the bug.
    expect(textA).not.toContain('REMOTE');
    expect(textB).not.toContain('REMOTE');

    await clientA.client.disconnect();
    await clientB.client.disconnect();
  });
});

// =========================================================================
// Suite 2: Verify that applyEditorOperations (splice) fixes the race.
// These tests fail without the splice fix (applyEditorOperations doesn't
// exist), serving as the regression tests.
// =========================================================================

describe('applyEditorOperations (splice fix)', () => {
  test('propagates to a second client', async () => {
    const clientA = createTrackedClient();
    const result = await clientA.client.createNewProject({
      syncServer: server.url,
      files: [{ path: 'doc.qmd', content: 'Hello world', contentType: 'text' }],
    });

    const clientB = createTrackedClient();
    await clientB.client.connect(server.url, result.indexDocId);
    await clientB.waitForFile('doc.qmd');

    expect(clientB.files.get('doc.qmd')).toBe('Hello world');

    const changePromise = clientB.waitForChange('doc.qmd');
    clientA.client.applyEditorOperations('doc.qmd', [
      { rangeOffset: 5, rangeLength: 0, text: ', beautiful' },
    ]);

    const receivedText = await changePromise;
    expect(receivedText).toBe('Hello, beautiful world');

    await clientA.client.disconnect();
    await clientB.client.disconnect();
  });

  test('concurrent edits from both clients preserve all text', async () => {
    const clientA = createTrackedClient();
    const result = await clientA.client.createNewProject({
      syncServer: server.url,
      files: [{ path: 'doc.qmd', content: 'Hello world', contentType: 'text' }],
    });

    const clientB = createTrackedClient();
    await clientB.client.connect(server.url, result.indexDocId);
    await clientB.waitForFile('doc.qmd');

    expect(clientB.files.get('doc.qmd')).toBe('Hello world');

    // Both clients edit concurrently
    clientA.client.applyEditorOperations('doc.qmd', [
      { rangeOffset: 5, rangeLength: 0, text: ', beautiful' },
    ]);
    clientB.client.applyEditorOperations('doc.qmd', [
      { rangeOffset: 11, rangeLength: 0, text: '!' },
    ]);

    // Wait for both clients to see both edits
    const hasBoth = (t: string) => t.includes(', beautiful') && t.includes('!');
    const [textA, textB] = await Promise.all([
      clientA.waitForContent('doc.qmd', hasBoth),
      clientB.waitForContent('doc.qmd', hasBoth),
    ]);

    expect(textA).toBe(textB);

    await clientA.client.disconnect();
    await clientB.client.disconnect();
  });

  test('stale offset preserves remote edit (unlike updateFileContent)', async () => {
    const clientA = createTrackedClient();
    const result = await clientA.client.createNewProject({
      syncServer: server.url,
      files: [{ path: 'doc.qmd', content: 'Hello world', contentType: 'text' }],
    });

    const clientB = createTrackedClient();
    await clientB.client.connect(server.url, result.indexDocId);
    await clientB.waitForFile('doc.qmd');

    // Client A inserts "REMOTE " at the start
    const changePromise = clientB.waitForChange('doc.qmd');
    clientA.client.applyEditorOperations('doc.qmd', [
      { rangeOffset: 0, rangeLength: 0, text: 'REMOTE ' },
    ]);
    await changePromise;

    expect(clientB.files.get('doc.qmd')).toBe('REMOTE Hello world');

    // Client B's editor was stale. Splice inserts "!" at offset 11.
    clientB.client.applyEditorOperations('doc.qmd', [
      { rangeOffset: 11, rangeLength: 0, text: '!' },
    ]);

    // Wait for A to see the "!"
    const textA = await clientA.waitForContent('doc.qmd', (t) => t.includes('!'));
    const textB = clientB.client.getFileContent('doc.qmd');

    // The "!" lands at offset 11 inside the CRDT ("REMOTE Hello world"),
    // splitting "Hello" into "Hell!o" — positionally wrong, but no text is
    // destroyed. The key property: every character from both peers survives.
    expect(textA).toContain('REMOTE');
    expect(textA).toContain('world');
    expect(textA).toContain('!');
    expect(textA).toHaveLength('REMOTE Hello world'.length + 1);
    expect(textB).toBe(textA); // converged

    await clientA.client.disconnect();
    await clientB.client.disconnect();
  });

  test('batch splice operations propagate correctly', async () => {
    const clientA = createTrackedClient();
    const result = await clientA.client.createNewProject({
      syncServer: server.url,
      files: [{ path: 'doc.qmd', content: 'foo bar foo baz foo', contentType: 'text' }],
    });

    const clientB = createTrackedClient();
    await clientB.client.connect(server.url, result.indexDocId);
    await clientB.waitForFile('doc.qmd');

    clientA.client.applyEditorOperations('doc.qmd', [
      { rangeOffset: 16, rangeLength: 3, text: 'qux' },
      { rangeOffset: 8, rangeLength: 3, text: 'qux' },
      { rangeOffset: 0, rangeLength: 3, text: 'qux' },
    ]);

    const textB = await clientB.waitForContent(
      'doc.qmd',
      (t) => !t.includes('foo'),
    );
    expect(textB).toBe('qux bar qux baz qux');

    await clientA.client.disconnect();
    await clientB.client.disconnect();
  });
});
