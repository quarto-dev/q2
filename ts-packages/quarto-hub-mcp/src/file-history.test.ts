/**
 * Integration tests for `get_file_history` (CAP-9) — Phase 3
 * (claude-notes/plans/2026-10-05-elevate-quarto-hub-mcp.md, bd-3qe7unp7).
 *
 * Drives the real server through the in-memory SDK fixture; a second
 * sync client with an author id + screen name plays the attributed
 * collaborator, so author-footer resolution (change.author ?? actor's
 * seq-1 footer ?? actor) and the index `identities` map are exercised
 * for real.
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

function parse(result: CallToolResult): Record<string, unknown> {
  const block = result.content[0];
  if (block?.type !== 'text') throw new Error('expected a text result block');
  return JSON.parse(block.text) as Record<string, unknown>;
}

interface HistoryEntry {
  head: string;
  hash: string;
  seq: number;
  time: number;
  author: string;
  name: string | null;
  color: string | null;
  added_chars: number;
  removed_chars: number;
}

/** Author IDs are actor-id-shaped: 64 lowercase hex chars (setAuthor rejects anything else). */
const CORA_AUTHOR_ID = 'c0ffee00'.repeat(8);

/** A collaborator whose changes carry an author footer + identity. */
async function startAttributedCollaborator(
  hubUrl: string,
  indexDocId: string,
): Promise<SyncClient> {
  const client = createSyncClient({
    onFileAdded() {},
    onFileChanged() {},
    onBinaryChanged() {},
    onFileRemoved() {},
  });
  await client.connect(hubUrl, indexDocId, CORA_AUTHOR_ID, 'Cora', '#AABBCC', {
    requireOnline: true,
    peerTimeoutMs: 8000,
  });
  return client;
}

/** Poll get_file_history until the server has seen `minEntries` changes. */
async function historyWhenReady(
  f: InMemoryMcpFixture,
  project: string,
  path: string,
  minEntries: number,
): Promise<HistoryEntry[]> {
  let entries: HistoryEntry[] = [];
  await vi.waitFor(
    async () => {
      const out = parse(await callTool(f, 'get_file_history', { project, path, limit: 100 }));
      entries = out.entries as HistoryEntry[];
      expect(entries.length).toBeGreaterThanOrEqual(minEntries);
    },
    { timeout: 10000, interval: 100 },
  );
  return entries;
}

describe('get_file_history — list mode (CAP-9)', () => {
  it('returns ordered change summaries with author attribution', async () => {
    const f = await startInMemoryMcp();
    let collab: SyncClient | undefined;
    try {
      const seed = await seedProject(f, [{ path: 'notes.txt', content: 'hello\n' }]);
      collab = await startAttributedCollaborator(f.hub.url, seed.indexDocId);

      collab.updateFileContent('notes.txt', 'hello world\n');
      collab.updateFileContent('notes.txt', 'hello brave world\n');

      const entries = await historyWhenReady(f, seed.indexDocId, 'notes.txt', 2);

      // Newest first: the second collaborator edit leads.
      expect(entries[0].author).toBe(CORA_AUTHOR_ID);
      expect(entries[0].name).toBe('Cora');
      expect(entries[0].color).toBe('#AABBCC');
      expect(entries[0].hash).toBe(
        hashPayload({ type: 'text', text: 'hello brave world\n' }),
      );
      // Minimal-splice stats: 'brave ' inserted, nothing removed.
      expect(entries[0].added_chars).toBe(6);
      expect(entries[0].removed_chars).toBe(0);

      // The first collaborator edit follows, same actor at a lower seq.
      expect(entries[1].author).toBe(CORA_AUTHOR_ID);
      expect(entries[1].name).toBe('Cora');
      expect(entries[1].seq).toBeLessThan(entries[0].seq);

      // Every entry carries the discovery fields a follow-up call needs.
      expect(entries[0].head).toMatch(/^[0-9a-f]{64}$|^[1-9A-HJ-NP-Za-km-z]+$/);
      expect(entries[0].time).toBeGreaterThan(0);
    } finally {
      await collab?.disconnect();
      await f.close();
    }
  }, 30000);

  it('attributes authorless changes to their bare actor with a null identity', async () => {
    const f = await startInMemoryMcp();
    try {
      // The fixture server connects authorless (no OAuth env), so its
      // own write_file lands with no author footer and no identity.
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        content: 'v2\n',
      });

      const out = parse(
        await callTool(f, 'get_file_history', { project: seed.indexDocId, path: 'a.qmd' }),
      );
      const entries = out.entries as HistoryEntry[];
      expect(entries[0].author.length).toBeGreaterThan(0);
      expect(entries[0].name).toBeNull();
      expect(entries[0].hash).toBe(hashPayload({ type: 'text', text: 'v2\n' }));
    } finally {
      await f.close();
    }
  }, 30000);

  it('bounds the listing by limit and reports truncation', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        content: 'v2\n',
      });
      await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        content: 'v3\n',
      });

      const out = parse(
        await callTool(f, 'get_file_history', {
          project: seed.indexDocId,
          path: 'a.qmd',
          limit: 2,
        }),
      );
      const entries = out.entries as HistoryEntry[];
      expect(entries).toHaveLength(2);
      expect(entries[0].hash).toBe(hashPayload({ type: 'text', text: 'v3\n' }));
      expect(entries[1].hash).toBe(hashPayload({ type: 'text', text: 'v2\n' }));
      expect(out.truncated).toBe(true);
      expect(out.total_changes).toBeGreaterThanOrEqual(3);
      expect(Array.isArray(out.heads)).toBe(true);
    } finally {
      await f.close();
    }
  }, 30000);
});

describe('get_file_history — diff mode (CAP-9)', () => {
  it('returns a unified diff between from_hash and to_hash', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [
        { path: 'notes.txt', content: 'alpha\nbeta\ngamma\n' },
      ]);
      await callTool(f, 'patch_file', {
        project: seed.indexDocId,
        path: 'notes.txt',
        old_string: 'beta',
        new_string: 'BETA',
      });

      const entries = await historyWhenReady(f, seed.indexDocId, 'notes.txt', 2);
      const fromHead = entries[entries.length - 1].head; // doc's first change
      const toHead = entries[0].head; // latest change

      const out = parse(
        await callTool(f, 'get_file_history', {
          project: seed.indexDocId,
          path: 'notes.txt',
          from_hash: fromHead,
          to_hash: toHead,
        }),
      );
      expect(out.from_hash).toBe(fromHead);
      expect(out.to_hash).toBe(toHead);
      expect(out.added_lines).toBe(1);
      expect(out.removed_lines).toBe(1);
      expect(out.diff).toBe(
        '--- a/notes.txt\n' +
          '+++ b/notes.txt\n' +
          '@@ -1,3 +1,3 @@\n' +
          ' alpha\n' +
          '-beta\n' +
          '+BETA\n' +
          ' gamma\n',
      );
    } finally {
      await f.close();
    }
  }, 30000);

  it('defaults to_hash to the current head when only from_hash is given', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'one\n' }]);
      await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        content: 'one\ntwo\n',
      });
      const entries = await historyWhenReady(f, seed.indexDocId, 'a.qmd', 2);

      const out = parse(
        await callTool(f, 'get_file_history', {
          project: seed.indexDocId,
          path: 'a.qmd',
          from_hash: entries[entries.length - 1].head,
        }),
      );
      expect(out.to_hash).toBe(entries[0].head);
      expect(out.diff).toContain('+two');
    } finally {
      await f.close();
    }
  }, 30000);

  it('rejects an unknown change hash with an actionable error', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'get_file_history', {
        project: seed.indexDocId,
        path: 'a.qmd',
        from_hash: 'not-a-known-change-hash',
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type !== 'text') throw new Error('expected a text result block');
      expect(block.text).toMatch(/not-a-known-change-hash/);
      expect(block.text).toMatch(/get_file_history/);
    } finally {
      await f.close();
    }
  }, 30000);

  it('rejects a binary file with an actionable error', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'x\n' }]);
      const png = Buffer.from([0x89, 0x50, 0x4e, 0x47]).toString('base64');
      await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'img.png',
        content: png,
        encoding: 'base64',
      });
      const result = await callTool(f, 'get_file_history', {
        project: seed.indexDocId,
        path: 'img.png',
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type !== 'text') throw new Error('expected a text result block');
      expect(block.text).toMatch(/binary/i);
    } finally {
      await f.close();
    }
  }, 30000);

  it('rejects a missing file with the not-found guidance', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'get_file_history', {
        project: seed.indexDocId,
        path: 'nope.qmd',
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type !== 'text') throw new Error('expected a text result block');
      expect(block.text).toMatch(/File not found: "nope\.qmd"/);
      expect(block.text).toMatch(/Closest existing paths/);
    } finally {
      await f.close();
    }
  }, 30000);
});
