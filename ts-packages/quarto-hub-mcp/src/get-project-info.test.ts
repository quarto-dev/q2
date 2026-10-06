/**
 * `get_project_info` (CAP-2): the "doctor" tool — one call answers
 * "what is this project and is my connection healthy": file/folder
 * counts, contributor identities, engine-capture state, index doc id,
 * server URL, auth mode, sync diagnostics, and (CAP-1) the share URL.
 */

import { describe, it, expect } from 'vitest';
import { generateAutomergeUrl, parseAutomergeUrl } from '@automerge/automerge-repo';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';
import { parseProjectRef } from './share-url.js';

interface ProjectInfo {
  project: string;
  server: string;
  shareUrl: string;
  auth_mode: string;
  counts: { files: number; binary: number; folders: number; unavailable: number };
  identities: Record<string, { name: string; color: string }>;
  captures: Record<string, { captureDocId: string; state?: string; lastError?: string }>;
  sync: { connected_peers: number; retry_timer_active: boolean; stranded: unknown[] };
}

async function info(f: InMemoryMcpFixture, project: string): Promise<ProjectInfo> {
  const result = await callTool(f, 'get_project_info', { project });
  expect(result.isError).not.toBe(true);
  return result.structuredContent as unknown as ProjectInfo;
}

describe('get_project_info (CAP-2)', () => {
  it('reports counts, ids, server, auth mode, and sync diagnostics', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [
        { path: 'index.qmd', content: 'x\n' },
        { path: 'a/b.qmd', content: 'y\n' },
      ]);
      await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'logo.png',
        content: Buffer.from([1, 2, 3]).toString('base64'),
        encoding: 'base64',
      });
      await callTool(f, 'create_folder', { project: seed.indexDocId, path: 'assets' });

      const got = await info(f, seed.indexDocId);
      expect(got.project).toBe(seed.indexDocId);
      expect(got.server).toBe(f.hub.url);
      expect(parseProjectRef(got.shareUrl).project).toBe(seed.indexDocId);
      expect(got.auth_mode).toBe('no-auth');
      expect(got.counts).toEqual({ files: 3, binary: 1, folders: 1, unavailable: 0 });
      expect(got.identities).toEqual({});
      expect(got.captures).toEqual({});
      expect(got.sync.connected_peers).toBeGreaterThanOrEqual(1);
      expect(got.sync.retry_timer_active).toBe(false);
      expect(got.sync.stranded).toEqual([]);
    } finally {
      await f.close();
    }
  });

  it('surfaces identities and engine-capture state from the index document', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      // Seed the sidecars directly through the index handle (an engine
      // capture normally arrives from the render path; a contributor
      // identity from a collaborator's connect).
      const state = await f.manager.connect(seed.indexDocId);
      state.client.getIndexHandle()!.change((d) => {
        d.identities = { actor1: { name: 'Charlie', color: '#E91E63' } };
        d.captures = {
          'index.qmd': { captureDocId: 'cap1', state: 'error', lastError: 'knitr blew up' },
        };
      });

      const got = await info(f, seed.indexDocId);
      expect(got.identities).toEqual({ actor1: { name: 'Charlie', color: '#E91E63' } });
      expect(got.captures['index.qmd']).toMatchObject({
        captureDocId: 'cap1',
        state: 'error',
        lastError: 'knitr blew up',
      });
    } finally {
      await f.close();
    }
  });

  it('counts unavailable (dangling) entries', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      // Mint a dangling entry: index references a doc the hub never got
      // (a freshly generated, therefore never-served, valid doc id).
      const ghostId = parseAutomergeUrl(generateAutomergeUrl()).documentId;
      const state = await f.manager.connect(seed.indexDocId);
      state.client.getIndexHandle()!.change((d) => {
        d.files['ghost.qmd'] = ghostId as never;
      });
      // The sync client marks the entry unavailable after its bounded
      // find attempts play out (seconds) — poll rather than sleep a
      // fixed guess.
      const deadline = Date.now() + 30000;
      while (state.client.getUnavailableFiles().length === 0) {
        if (Date.now() > deadline) {
          throw new Error('ghost entry was never marked unavailable');
        }
        await new Promise((r) => setTimeout(r, 250));
      }
      const got = await info(f, seed.indexDocId);
      expect(got.counts.unavailable).toBe(1);
    } finally {
      await f.close();
    }
  }, 20000);
});
