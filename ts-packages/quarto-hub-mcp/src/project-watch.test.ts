/**
 * Integration tests for the project-wide `wait_for_change` arm (CAP-18,
 * ERG-8) and its progress notifications (BP-4) — Phase 3
 * (claude-notes/plans/2026-10-05-elevate-quarto-hub-mcp.md, bd-3qe7unp7).
 *
 * Drives the real server through the in-memory SDK fixture
 * (in-memory-fixture.ts); collaborator edits come from a second, real
 * sync client connected to the same test-hub, so the watcher's events
 * travel the same sync path production uses.
 *
 * Pinned behavior:
 *   - `path` omitted → watch the whole project; the result is
 *     `{ changed, changes: [{ path, hash, kind }] }` with kind
 *     'added' | 'edited' | 'removed'.
 *   - `since_hash` excludes the agent's own write echo: a change whose
 *     content hash equals `since_hash` never fires nor joins the
 *     reported set (ERG-8).
 *   - Cancellation unregisters the project waiter promptly (BP-3's
 *     hygiene extended to the project-wide arm).
 *   - disconnect_project interrupts a pending project-wide poll with an
 *     honest error, never a hang (HY-5 parity with per-path polls).
 *   - A `progressToken` produces `notifications/progress` over the wait
 *     (BP-4).
 */

import { describe, it, expect, vi } from 'vitest';
import type { CallToolResult } from '@modelcontextprotocol/client';
import {
  createSyncClient,
  type SyncClient,
} from '@quarto/quarto-sync-client';

import { hashPayload } from './connection-manager.js';
import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';

/** A second sync client playing the human collaborator. */
async function startCollaborator(
  hubUrl: string,
  indexDocId: string,
): Promise<SyncClient> {
  const client = createSyncClient({
    onFileAdded() {},
    onFileChanged() {},
    onBinaryChanged() {},
    onFileRemoved() {},
  });
  await client.connect(hubUrl, indexDocId, undefined, undefined, undefined, {
    requireOnline: true,
    peerTimeoutMs: 8000,
  });
  return client;
}

function parse(result: CallToolResult): Record<string, unknown> {
  const block = result.content[0];
  if (block?.type !== 'text') throw new Error('expected a text result block');
  return JSON.parse(block.text) as Record<string, unknown>;
}

interface ChangeEntry {
  path: string;
  hash: string | null;
  kind: 'added' | 'edited' | 'removed';
}

describe('wait_for_change — project-wide arm (CAP-18)', () => {
  it('reports a collaborator edit as [{ path, hash, kind: "edited" }]', async () => {
    const f = await startInMemoryMcp();
    let collab: SyncClient | undefined;
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      collab = await startCollaborator(f.hub.url, seed.indexDocId);

      const waitCall = callTool(f, 'wait_for_change', {
        project: seed.indexDocId,
        timeout_seconds: 10,
      });
      // The waiter must be armed before the collaborator's edit lands.
      await vi.waitFor(
        () => {
          expect(f.manager.pendingProjectWaiterCount(seed.indexDocId)).toBe(1);
        },
        { timeout: 10000, interval: 25 },
      );

      collab.updateFileContent('a.qmd', 'v2 from collaborator\n');

      const out = parse(await waitCall);
      expect(out.changed).toBe(true);
      const changes = out.changes as ChangeEntry[];
      expect(changes).toHaveLength(1);
      expect(changes[0].path).toBe('a.qmd');
      expect(changes[0].kind).toBe('edited');
      expect(changes[0].hash).toBe(
        hashPayload({ type: 'text', text: 'v2 from collaborator\n' }),
      );
    } finally {
      await collab?.disconnect();
      await f.close();
    }
  }, 30000);

  it('reports a collaborator file creation as kind "added"', async () => {
    const f = await startInMemoryMcp();
    let collab: SyncClient | undefined;
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      collab = await startCollaborator(f.hub.url, seed.indexDocId);

      const waitCall = callTool(f, 'wait_for_change', {
        project: seed.indexDocId,
        timeout_seconds: 10,
      });
      await vi.waitFor(
        () => {
          expect(f.manager.pendingProjectWaiterCount(seed.indexDocId)).toBe(1);
        },
        { timeout: 10000, interval: 25 },
      );

      await collab.createFile('new.qmd', 'fresh\n');

      const out = parse(await waitCall);
      expect(out.changed).toBe(true);
      const changes = out.changes as ChangeEntry[];
      expect(changes.map((c) => [c.path, c.kind])).toEqual([['new.qmd', 'added']]);
      expect(changes[0].hash).toBe(hashPayload({ type: 'text', text: 'fresh\n' }));
    } finally {
      await collab?.disconnect();
      await f.close();
    }
  }, 30000);

  it('reports a collaborator file removal as kind "removed" with a null hash', async () => {
    const f = await startInMemoryMcp();
    let collab: SyncClient | undefined;
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      collab = await startCollaborator(f.hub.url, seed.indexDocId);

      const waitCall = callTool(f, 'wait_for_change', {
        project: seed.indexDocId,
        timeout_seconds: 10,
      });
      await vi.waitFor(
        () => {
          expect(f.manager.pendingProjectWaiterCount(seed.indexDocId)).toBe(1);
        },
        { timeout: 10000, interval: 25 },
      );

      collab.deleteFile('a.qmd');

      const out = parse(await waitCall);
      expect(out.changed).toBe(true);
      const changes = out.changes as ChangeEntry[];
      expect(changes).toEqual([{ path: 'a.qmd', kind: 'removed', hash: null }]);
    } finally {
      await collab?.disconnect();
      await f.close();
    }
  }, 30000);

  it("excludes the agent's own write echo when since_hash matches (ERG-8)", async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const ownHash = hashPayload({ type: 'text', text: 'v2 by the agent\n' });

      const waitCall = callTool(f, 'wait_for_change', {
        project: seed.indexDocId,
        timeout_seconds: 2,
        since_hash: ownHash,
      });
      await vi.waitFor(
        () => {
          expect(f.manager.pendingProjectWaiterCount(seed.indexDocId)).toBe(1);
        },
        { timeout: 10000, interval: 25 },
      );

      // The agent's own write lands while the watch is armed (parallel
      // tool calls) — its echo must NOT fire the watcher.
      const write = await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        content: 'v2 by the agent\n',
      });
      expect(write.isError).not.toBe(true);

      const out = parse(await waitCall);
      expect(out.changed).toBe(false);
      expect(out.changes).toEqual([]);
    } finally {
      await f.close();
    }
  }, 30000);

  it('reports the same own-write when since_hash is absent (exclusion control)', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);

      const waitCall = callTool(f, 'wait_for_change', {
        project: seed.indexDocId,
        timeout_seconds: 10,
      });
      await vi.waitFor(
        () => {
          expect(f.manager.pendingProjectWaiterCount(seed.indexDocId)).toBe(1);
        },
        { timeout: 10000, interval: 25 },
      );

      await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        content: 'v2 by the agent\n',
      });

      const out = parse(await waitCall);
      expect(out.changed).toBe(true);
      const changes = out.changes as ChangeEntry[];
      expect(changes.map((c) => c.path)).toContain('a.qmd');
    } finally {
      await f.close();
    }
  }, 30000);

  it('times out with changed:false and an empty changes array', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const out = parse(
        await callTool(f, 'wait_for_change', {
          project: seed.indexDocId,
          timeout_seconds: 1,
        }),
      );
      expect(out.changed).toBe(false);
      expect(out.changes).toEqual([]);
      expect(out.message).toContain('1s');
    } finally {
      await f.close();
    }
  }, 30000);

  it('keeps the per-path arm working when path is given', async () => {
    const f = await startInMemoryMcp();
    let collab: SyncClient | undefined;
    try {
      const seed = await seedProject(f, [
        { path: 'a.qmd', content: 'v1\n' },
        { path: 'b.qmd', content: 'b1\n' },
      ]);
      collab = await startCollaborator(f.hub.url, seed.indexDocId);

      const waitCall = callTool(f, 'wait_for_change', {
        project: seed.indexDocId,
        path: 'a.qmd',
        timeout_seconds: 10,
      });
      await vi.waitFor(
        () => {
          expect(f.manager.pendingWaiterCount(seed.indexDocId)).toBe(1);
        },
        { timeout: 10000, interval: 25 },
      );

      // An edit to the OTHER file must not fire the per-path watch…
      collab.updateFileContent('b.qmd', 'b2\n');
      // …and the watched file's edit must.
      collab.updateFileContent('a.qmd', 'a2\n');

      const out = parse(await waitCall);
      expect(out.changed).toBe(true);
      expect(out.path).toBe('a.qmd');
      expect(out.content).toBe('a2\n');
    } finally {
      await collab?.disconnect();
      await f.close();
    }
  }, 30000);

  it('cancelling a project-wide poll frees the waiter promptly (BP-3 parity)', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const controller = new AbortController();
      const call = f.client.callTool(
        {
          name: 'wait_for_change',
          arguments: { project: seed.indexDocId, timeout_seconds: 30 },
        },
        { signal: controller.signal },
      );
      await vi.waitFor(
        () => {
          expect(f.manager.pendingProjectWaiterCount(seed.indexDocId)).toBe(1);
        },
        { timeout: 10000, interval: 25 },
      );

      controller.abort();
      await expect(call).rejects.toThrow(/abort|cancel/i);
      await vi.waitFor(
        () => {
          expect(f.manager.pendingProjectWaiterCount(seed.indexDocId)).toBe(0);
        },
        { timeout: 1000, interval: 25 },
      );
    } finally {
      await f.close();
    }
  }, 30000);

  it('disconnect_project interrupts a pending project-wide poll (HY-5 parity)', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const waitCall = callTool(f, 'wait_for_change', {
        project: seed.indexDocId,
        timeout_seconds: 30,
      });
      await vi.waitFor(
        () => {
          expect(f.manager.pendingProjectWaiterCount(seed.indexDocId)).toBe(1);
        },
        { timeout: 10000, interval: 25 },
      );

      await callTool(f, 'disconnect_project', { project: seed.indexDocId });

      const out = await waitCall;
      expect(out.isError).toBe(true);
      const block = out.content[0];
      if (block?.type !== 'text') throw new Error('expected a text result block');
      expect(block.text).toMatch(/disconnected/);
    } finally {
      await f.close();
    }
  }, 30000);
});

describe('wait_for_change — progress notifications (BP-4)', () => {
  it('emits notifications/progress when the caller passes a progressToken', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const progress: Array<{ progress: number; total?: number; message?: string }> = [];
      const result = await f.client.callTool(
        {
          name: 'wait_for_change',
          arguments: { project: seed.indexDocId, timeout_seconds: 2 },
        },
        {
          onprogress: (p) => {
            progress.push(p);
          },
        },
      );
      expect(result.isError).not.toBe(true);
      // At minimum the immediate progress=0 emission; periodic emissions
      // (every 5s of a longer wait) are covered handler-level with fake
      // timers.
      expect(progress.length).toBeGreaterThanOrEqual(1);
      expect(progress[0].progress).toBe(0);
      expect(progress[0].total).toBe(2000);
    } finally {
      await f.close();
    }
  }, 30000);
});
