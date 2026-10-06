/**
 * Integration tests for `restore_file_version` (CAP-19, Q-6) — Phase 3
 * (claude-notes/plans/2026-10-05-elevate-quarto-hub-mcp.md, bd-3qe7unp7).
 *
 * A restore is a NEW, attributed change whose content equals the
 * historical text (CRDT-safe — history is never rewritten), and the
 * result carries what a reversal needs: the pre-restore content hash
 * and heads. Rides CAP-9's heads/view plumbing.
 */

import { describe, it, expect } from 'vitest';
import type { CallToolResult } from '@modelcontextprotocol/client';

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
}

async function history(
  f: InMemoryMcpFixture,
  project: string,
  path: string,
): Promise<HistoryEntry[]> {
  const out = parse(await callTool(f, 'get_file_history', { project, path, limit: 100 }));
  return out.entries as HistoryEntry[];
}

async function readText(
  f: InMemoryMcpFixture,
  project: string,
  path: string,
): Promise<string> {
  const out = parse(await callTool(f, 'read_file', { project, path }));
  return out.content as string;
}

describe('restore_file_version (CAP-19)', () => {
  it('restores a prior version as a new change and carries the pre-restore hash', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const project = seed.indexDocId;
      await callTool(f, 'write_file', { project, path: 'a.qmd', content: 'v2\n' });
      await callTool(f, 'write_file', { project, path: 'a.qmd', content: 'v3\n' });

      const entries = await history(f, project, 'a.qmd');
      // Newest-first: [v3-write, v2-write, v1-create].
      const v1Head = entries[2].head;
      expect(entries[2].hash).toBe(hashPayload({ type: 'text', text: 'v1\n' }));

      const out = parse(
        await callTool(f, 'restore_file_version', { project, path: 'a.qmd', hash: v1Head }),
      );
      expect(out.restored_from).toBe(v1Head);
      expect(out.pre_restore_hash).toBe(hashPayload({ type: 'text', text: 'v3\n' }));
      expect(out.pre_restore_heads).toEqual([entries[0].head]);
      expect(out.hash).toBe(hashPayload({ type: 'text', text: 'v1\n' }));
      expect(out.synced).toBe(true);

      // The content is the historical text, as a NEW change on top.
      expect(await readText(f, project, 'a.qmd')).toBe('v1\n');
      const after = await history(f, project, 'a.qmd');
      expect(after.length).toBe(entries.length + 1);
      expect(after[0].hash).toBe(hashPayload({ type: 'text', text: 'v1\n' }));
    } finally {
      await f.close();
    }
  }, 30000);

  it('is itself reversible via pre_restore_heads', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const project = seed.indexDocId;
      await callTool(f, 'write_file', { project, path: 'a.qmd', content: 'v2\n' });

      const entries = await history(f, project, 'a.qmd');
      const restored = parse(
        await callTool(f, 'restore_file_version', {
          project,
          path: 'a.qmd',
          hash: entries[1].head,
        }),
      );
      expect(await readText(f, project, 'a.qmd')).toBe('v1\n');

      const heads = restored.pre_restore_heads as string[];
      expect(heads).toHaveLength(1);
      const undone = parse(
        await callTool(f, 'restore_file_version', { project, path: 'a.qmd', hash: heads[0] }),
      );
      expect(undone.restored_from).toBe(heads[0]);
      expect(await readText(f, project, 'a.qmd')).toBe('v2\n');
    } finally {
      await f.close();
    }
  }, 30000);

  it('refuses to restore over a racing collaborator edit when expected_hash is stale', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const project = seed.indexDocId;
      await callTool(f, 'write_file', { project, path: 'a.qmd', content: 'v2\n' });
      const entries = await history(f, project, 'a.qmd');

      const result = await callTool(f, 'restore_file_version', {
        project,
        path: 'a.qmd',
        hash: entries[1].head,
        expected_hash: hashPayload({ type: 'text', text: 'stale\n' }),
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type !== 'text') throw new Error('expected a text result block');
      expect(block.text).toMatch(/refused|stale|expected_hash/i);
      // Untouched.
      expect(await readText(f, project, 'a.qmd')).toBe('v2\n');
    } finally {
      await f.close();
    }
  }, 30000);

  it('is a no-op (no new change) when the content already equals the target', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const project = seed.indexDocId;
      await callTool(f, 'write_file', { project, path: 'a.qmd', content: 'v2\n' });
      const entries = await history(f, project, 'a.qmd');

      const out = parse(
        await callTool(f, 'restore_file_version', {
          project,
          path: 'a.qmd',
          hash: entries[0].head, // the current head
        }),
      );
      expect(out.already_current).toBe(true);
      expect(await readText(f, project, 'a.qmd')).toBe('v2\n');
      const after = await history(f, project, 'a.qmd');
      expect(after.length).toBe(entries.length);
    } finally {
      await f.close();
    }
  }, 30000);

  it('rejects an unknown change hash with an actionable error', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'restore_file_version', {
        project: seed.indexDocId,
        path: 'a.qmd',
        hash: 'not-a-known-change-hash',
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
      const result = await callTool(f, 'restore_file_version', {
        project: seed.indexDocId,
        path: 'img.png',
        hash: 'whatever',
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type !== 'text') throw new Error('expected a text result block');
      expect(block.text).toMatch(/binary/i);
    } finally {
      await f.close();
    }
  }, 30000);
});
