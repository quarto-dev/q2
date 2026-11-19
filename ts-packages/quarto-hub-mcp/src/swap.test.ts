/**
 * GE-7 test specification 7: after the project owner replaces a file (a swap of `files[path]` to a different doc), the
 * `edit` tool with the pre-swap `expected_hash` is refused and returns the current content and hash.
 *
 * The MCP server needs no swap code of its own: the sync client rebinds the path and re-fires `onFileAdded` with the new
 * doc's text, the connection manager's file map takes it, and the existing compare-and-swap sees a different hash. This
 * test pins that, because the alternative (an agent overwriting the new doc with text computed from the old one) is the
 * failure the hash exists to prevent.
 */
import { describe, it, expect } from 'vitest';
import type { DocumentId } from '@automerge/automerge-repo';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';

function parseJson(result: { content: Array<{ type: string; text?: string }> }): Record<string, unknown> {
  const block = result.content[0];
  if (block?.type !== 'text' || typeof block.text !== 'string') {
    throw new Error(`expected a text content block, got ${JSON.stringify(block)}`);
  }
  return JSON.parse(block.text) as Record<string, unknown>;
}

/** The owner's side: a new doc on the hub's repo, and the flip of `files[path]` to it. */
async function flip(f: InMemoryMcpFixture, indexDocId: string, path: string, text: string): Promise<void> {
  const doc = f.hub.repo.create<{ text: string }>({ text });
  const index = await f.hub.repo.find<{ files: Record<string, string> }>(indexDocId as DocumentId);
  index.change((d) => {
    d.files[path] = doc.documentId;
  });
}

async function readUntil(
  f: InMemoryMcpFixture,
  project: string,
  path: string,
  content: string,
): Promise<Record<string, unknown>> {
  const deadline = Date.now() + 10000;
  for (;;) {
    const payload = parseJson(await callTool(f, 'read', { action: 'read', project, path }));
    if (payload.content === content) return payload;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${path} to read ${JSON.stringify(content)}`);
    await new Promise((r) => setTimeout(r, 50));
  }
}

describe('edit after the owner replaces a file (GE-7)', () => {
  it('refuses a write with the pre-swap expected_hash and returns the current content and hash', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'version A\n' }]);
      const before = parseJson(
        await callTool(f, 'read', { action: 'read', project: seed.indexDocId, path: 'a.qmd' }),
      );

      await flip(f, seed.indexDocId, 'a.qmd', 'version B\n');
      const after = await readUntil(f, seed.indexDocId, 'a.qmd', 'version B\n');
      expect(after.hash).not.toBe(before.hash);

      const refused = await callTool(f, 'edit', {
        action: 'write',
        project: seed.indexDocId,
        path: 'a.qmd',
        content: 'overwrite computed from version A\n',
        expected_hash: before.hash,
      });
      expect(refused.isError).toBe(true);
      const payload = parseJson(refused);
      expect(payload.content).toBe('version B\n');
      expect(payload.hash).toBe(after.hash);

      // Nothing changed in the new doc.
      const unchanged = parseJson(
        await callTool(f, 'read', { action: 'read', project: seed.indexDocId, path: 'a.qmd' }),
      );
      expect(unchanged.content).toBe('version B\n');
    } finally {
      await f.close();
    }
  }, 30000);

  it('refuses a patch with the pre-swap expected_hash the same way', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'the qick fox\n' }]);
      const before = parseJson(
        await callTool(f, 'read', { action: 'read', project: seed.indexDocId, path: 'a.qmd' }),
      );
      await flip(f, seed.indexDocId, 'a.qmd', 'the qick fox, replaced\n');
      await readUntil(f, seed.indexDocId, 'a.qmd', 'the qick fox, replaced\n');

      const refused = await callTool(f, 'edit', {
        action: 'patch',
        project: seed.indexDocId,
        path: 'a.qmd',
        old_string: 'qick',
        new_string: 'quick',
        expected_hash: before.hash,
      });
      expect(refused.isError).toBe(true);
      expect(parseJson(refused).content).toBe('the qick fox, replaced\n');
    } finally {
      await f.close();
    }
  }, 30000);

  it('accepts a write with the post-swap hash', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'A\n' }]);
      await flip(f, seed.indexDocId, 'a.qmd', 'B\n');
      const current = await readUntil(f, seed.indexDocId, 'a.qmd', 'B\n');

      const ok = await callTool(f, 'edit', {
        action: 'write',
        project: seed.indexDocId,
        path: 'a.qmd',
        content: 'B, edited\n',
        expected_hash: current.hash,
      });
      expect(ok.isError).not.toBe(true);
    } finally {
      await f.close();
    }
  }, 30000);
});
