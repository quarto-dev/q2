/**
 * `disconnect_project` (HY-5): per-project teardown. A long agent
 * session touching many projects accumulates websocket + in-memory-doc
 * state per project; this is the release valve. Disconnect drains
 * outbound sync first (bounded) so nothing the agent wrote is stranded,
 * interrupts pending wait_for_change polls honestly, and a later tool
 * call transparently reconnects.
 */

import { describe, it, expect } from 'vitest';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
} from './in-memory-fixture.js';

describe('disconnect_project (HY-5)', () => {
  it('disconnects a connected project; the next call transparently reconnects', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'disconnect_project', { project: seed.indexDocId });
      expect(result.isError).not.toBe(true);
      expect(result.structuredContent).toMatchObject({
        project: seed.indexDocId,
        disconnected: true,
        synced: true,
      });

      // Reconnect is transparent.
      const read = await callTool(f, 'read_file', {
        project: seed.indexDocId,
        path: 'index.qmd',
      });
      expect(read.isError).not.toBe(true);
      expect((read.structuredContent as { content?: string }).content).toBe('x\n');
    } finally {
      await f.close();
    }
  });

  it('wait_for_sync: false skips the delivery wait (no synced field)', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'disconnect_project', {
        project: seed.indexDocId,
        wait_for_sync: false,
      });
      expect(result.isError).not.toBe(true);
      expect(result.structuredContent).toMatchObject({ disconnected: true });
      expect((result.structuredContent as { synced?: unknown }).synced).toBeUndefined();
    } finally {
      await f.close();
    }
  });

  it('not-connected is an actionable error naming the project', async () => {
    const f = await startInMemoryMcp();
    try {
      const result = await callTool(f, 'disconnect_project', { project: 'neverconnected' });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type === 'text') {
        expect(block.text).toMatch(/neverconnected/);
        expect(block.text).toMatch(/not connected/i);
      }
    } finally {
      await f.close();
    }
  });

  it('a pending wait_for_change settles with an honest interruption', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const pending = f.client.callTool({
        name: 'wait_for_change',
        arguments: { project: seed.indexDocId, path: 'index.qmd', timeout_seconds: 30 },
      });
      // Let the poll register its waiter before the disconnect lands.
      await new Promise((r) => setTimeout(r, 300));
      expect(f.manager.pendingWaiterCount(seed.indexDocId)).toBe(1);

      await callTool(f, 'disconnect_project', { project: seed.indexDocId });
      const settled = await pending;
      expect(settled.isError).toBe(true);
      const block = settled.content[0];
      if (block?.type === 'text') {
        expect(block.text).toMatch(/disconnect/);
      }
      expect(f.manager.pendingWaiterCount(seed.indexDocId)).toBe(0);
    } finally {
      await f.close();
    }
  });
});
