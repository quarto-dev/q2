/**
 * `createFileIfAbsent` (document import, I11/I20/I22): refuses an occupied
 * path without touching the index, and creates a new text file with its whole
 * content as the document's single initial change.
 *
 * Runs against the in-process JS test hub (no `target/debug/hub` needed), so
 * it always runs rather than skipping.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { getAllChanges, type Doc } from '@automerge/automerge';

import { createSyncClient, type SyncClient } from './client.js';
import { startTestHub, type TestHub } from './test-hub.js';

let hub: TestHub;
let c: SyncClient;
const added: Array<{ path: string; text?: string }> = [];

beforeEach(async () => {
  hub = await startTestHub();
  added.length = 0;
  c = createSyncClient({
    onFileAdded: (path, payload) =>
      added.push({ path, text: payload.type === 'text' ? payload.text : undefined }),
    onFileChanged: () => {},
    onFileRemoved: () => {},
  });
  await c.createNewProject({
    syncServer: hub.url,
    files: [{ path: 'existing.qmd', content: 'original\n', contentType: 'text' }],
    storage: 'memory',
    peerTimeoutMs: 10000,
    requireOnline: true,
  });
  added.length = 0;
});

afterEach(async () => {
  await c.disconnect();
  await hub.stop();
});

describe('createFileIfAbsent', () => {
  it('creates a new file with its text as the first and only change', async () => {
    const r = await c.createFileIfAbsent('notes/new.qmd', '# Imported\n\nbody\n');
    expect(r.created).toBe(true);
    expect(r.docId).toBeTruthy();

    const index = c.getIndexHandle()!.doc()!;
    expect(index.files['notes/new.qmd']).toBe(r.docId);

    const handle = c.getFileHandle('notes/new.qmd')!;
    expect((handle.doc() as { text: string }).text).toBe('# Imported\n\nbody\n');
    // One change, not an empty document plus a second change.
    expect(getAllChanges(handle.doc() as Doc<unknown>)).toHaveLength(1);

    expect(added).toEqual([{ path: 'notes/new.qmd', text: '# Imported\n\nbody\n' }]);
    expect(c.getFilePaths()).toContain('notes/new.qmd');
  });

  it('normalizes the path like createFile', async () => {
    const r = await c.createFileIfAbsent('/a//b.qmd', 'x');
    expect(r.created).toBe(true);
    expect(c.getIndexHandle()!.doc()!.files['a/b.qmd']).toBe(r.docId);
  });

  it('refuses an occupied path and changes nothing', async () => {
    const before = c.getIndexHandle()!.doc()!.files['existing.qmd'];
    const r = await c.createFileIfAbsent('existing.qmd', 'replacement\n');
    expect(r).toEqual({ created: false });
    expect(r.docId).toBeUndefined();

    expect(c.getIndexHandle()!.doc()!.files['existing.qmd']).toBe(before);
    expect((c.getFileHandle('existing.qmd')!.doc() as { text: string }).text).toBe('original\n');
    expect(added).toEqual([]);
  });

  it('refuses the second of two creates at the same path', async () => {
    const first = await c.createFileIfAbsent('dup.qmd', 'one');
    const second = await c.createFileIfAbsent('dup.qmd', 'two');
    expect(first.created).toBe(true);
    expect(second.created).toBe(false);
    expect((c.getFileHandle('dup.qmd')!.doc() as { text: string }).text).toBe('one');
  });

  it('throws when not connected', async () => {
    const fresh = createSyncClient({
      onFileAdded: () => {},
      onFileChanged: () => {},
      onFileRemoved: () => {},
    });
    await expect(fresh.createFileIfAbsent('x.qmd', '')).rejects.toThrow('Not connected');
  });
});
