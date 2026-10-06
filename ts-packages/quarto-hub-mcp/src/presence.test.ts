/**
 * Integration tests for `list_presence` (CAP-8, Q-3) — Phase 3
 * (claude-notes/plans/2026-10-05-elevate-quarto-hub-mcp.md, bd-3qe7unp7).
 *
 * The fake peer is the test-hub's own repo: broadcasting a hand-crafted
 * message per hub-client's presenceService schema on a file document
 * (or the index document) is exactly what a web client's editor does.
 * The MCP server must observe passively — and must itself NEVER emit
 * presence (Q-3: the server authenticates as the human; announcing a
 * fake cursor under their identity would lie to collaborators).
 */

import { describe, it, expect, vi } from 'vitest';
import { next as A } from '@automerge/automerge';
import type { DocHandle } from '@automerge/automerge-repo';
import type { FileDocumentContent } from '@quarto/quarto-sync-client';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';

interface PresenceEntry {
  peer_id: string;
  user_id: string;
  user_name: string;
  user_color: string;
  file_path: string | null;
  cursor_offset: number | null;
  selection: { start_offset: number; end_offset: number } | null;
  last_seen_ms_ago: number;
  active: boolean;
}

async function listPresence(
  f: InMemoryMcpFixture,
  project: string,
): Promise<PresenceEntry[]> {
  const result = await callTool(f, 'list_presence', { project });
  expect(result.isError).not.toBe(true);
  const block = result.content[0];
  if (block?.type !== 'text') throw new Error('expected a text result block');
  const out = JSON.parse(block.text) as { presences: PresenceEntry[] };
  return out.presences;
}

/** Broadcast until list_presence reflects the peer (sync delivery is async). */
async function waitForPeer(
  f: InMemoryMcpFixture,
  project: string,
  peerId: string,
): Promise<PresenceEntry> {
  let found: PresenceEntry | undefined;
  await vi.waitFor(
    async () => {
      const presences = await listPresence(f, project);
      found = presences.find((p) => p.peer_id === peerId);
      expect(found).toBeDefined();
    },
    { timeout: 8000, interval: 100 },
  );
  return found!;
}

const PEER = {
  type: 'presence' as const,
  peerId: 'peer-xavier',
  userId: 'user-xavier',
  userName: 'Xavier',
  userColor: '#E91E63',
};

describe('list_presence (CAP-8)', () => {
  it('reflects a fake peer broadcasting on a file document', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'hello world\n' }]);
      const docId = seed.files.find((x) => x.path === 'a.qmd')!.docId;
      const handle = await f.hub.repo.find<FileDocumentContent>(docId as never);

      // Cursor anchored hub-side at offset 6 — elemIds are intrinsic,
      // so the server's replica resolves it to the same offset.
      const cursor = A.getCursor(handle.doc()!, ['text'], 6);
      handle.broadcast({
        ...PEER,
        cursor,
        selection: null,
      });

      const entry = await waitForPeer(f, seed.indexDocId, PEER.peerId);
      expect(entry.user_name).toBe('Xavier');
      expect(entry.user_id).toBe('user-xavier');
      expect(entry.user_color).toBe('#E91E63');
      expect(entry.file_path).toBe('a.qmd');
      expect(entry.cursor_offset).toBe(6);
      expect(entry.selection).toBeNull();
      expect(entry.active).toBe(true);
      expect(entry.last_seen_ms_ago).toBeGreaterThanOrEqual(0);
    } finally {
      await f.close();
    }
  }, 30000);

  it('reflects a presence message on the index channel with file_path null', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'x\n' }]);
      const indexHandle = await f.hub.repo.find(seed.indexDocId as never);
      indexHandle.broadcast({ ...PEER, cursor: null, selection: null });

      const entry = await waitForPeer(f, seed.indexDocId, PEER.peerId);
      expect(entry.file_path).toBeNull();
      expect(entry.cursor_offset).toBeNull();
    } finally {
      await f.close();
    }
  }, 30000);

  it('drops the peer on a leave message', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'x\n' }]);
      const docId = seed.files.find((x) => x.path === 'a.qmd')!.docId;
      const handle = await f.hub.repo.find<FileDocumentContent>(docId as never);

      handle.broadcast({ ...PEER, cursor: null, selection: null });
      await waitForPeer(f, seed.indexDocId, PEER.peerId);

      handle.broadcast({ type: 'leave', peerId: PEER.peerId });
      await vi.waitFor(
        async () => {
          const presences = await listPresence(f, seed.indexDocId);
          expect(presences.find((p) => p.peer_id === PEER.peerId)).toBeUndefined();
        },
        { timeout: 8000, interval: 100 },
      );
    } finally {
      await f.close();
    }
  }, 30000);

  it('never emits presence of its own (Q-3)', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'x\n' }]);
      const docId = seed.files.find((x) => x.path === 'a.qmd')!.docId;
      const fileHandle: DocHandle<FileDocumentContent> = await f.hub.repo.find(docId as never);
      const indexHandle = await f.hub.repo.find(seed.indexDocId as never);

      // Hub-side wiretap: only two peers exist (hub + MCP server), so
      // ANY ephemeral message observed here came from the server.
      const seen: unknown[] = [];
      const tap = (payload: { message: unknown }) => {
        seen.push(payload.message);
      };
      fileHandle.on('ephemeral-message', tap);
      indexHandle.on('ephemeral-message', tap);

      await listPresence(f, seed.indexDocId);
      // A beat long enough for any broadcast to have arrived.
      await new Promise((r) => setTimeout(r, 500));
      expect(seen).toEqual([]);
    } finally {
      await f.close();
    }
  }, 30000);

  it('reports an empty list with guidance when nobody is around', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'list_presence', { project: seed.indexDocId });
      const block = result.content[0];
      if (block?.type !== 'text') throw new Error('expected a text result block');
      const out = JSON.parse(block.text) as { presences: PresenceEntry[]; message?: string };
      expect(out.presences).toEqual([]);
      expect(out.message).toMatch(/passive|no collaborators/i);
    } finally {
      await f.close();
    }
  }, 30000);
});
