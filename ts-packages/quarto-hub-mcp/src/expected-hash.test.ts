/**
 * Stale-view protection (ERG-1, bd-zv8u2sxi;
 * claude-notes/plans/2026-10-05-elevate-quarto-hub-mcp.md).
 *
 * A human editor sees the current text before typing; an agent that
 * read a file an hour ago does not. `read_file`/`write_file`/
 * `patch_file` results carry a `sha256:` content hash, and the write
 * tools accept `expected_hash` — compare-and-swap against the current
 * doc, so an agent can never silently revert a collaborator's edit
 * that landed between its read and its write.
 */

import { describe, it, expect } from 'vitest';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';

/** Parse the JSON text payload of a successful tool result. */
function parseJson(result: { content: Array<{ type: string; text?: string }> }): Record<string, unknown> {
  const block = result.content[0];
  if (block?.type !== 'text' || typeof block.text !== 'string') {
    throw new Error(`expected a text content block, got ${JSON.stringify(block)}`);
  }
  return JSON.parse(block.text) as Record<string, unknown>;
}

const HASH_RE = /^sha256:[0-9a-f]{64}$/;

describe('content hashes and compare-and-swap (ERG-1)', () => {
  it('read_file returns the file content with its sha256 hash', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'hello\n' }]);
      const result = await callTool(f, 'read_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
      });
      expect(result.isError).not.toBe(true);
      const payload = parseJson(result);
      expect(payload.path).toBe('a.qmd');
      expect(payload.content).toBe('hello\n');
      expect(payload.hash).toMatch(HASH_RE);
    } finally {
      await f.close();
    }
  });

  it('write_file returns the new hash; a matching expected_hash applies', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const read = parseJson(
        await callTool(f, 'read_file', { project: seed.indexDocId, path: 'a.qmd' }),
      );

      const write = await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        content: 'v2\n',
        expected_hash: read.hash,
      });
      expect(write.isError).not.toBe(true);
      const written = parseJson(write);
      expect(written.path).toBe('a.qmd');
      expect(written.hash).toMatch(HASH_RE);
      expect(written.hash).not.toBe(read.hash);

      // And the read-back hash matches what the write returned.
      const reread = parseJson(
        await callTool(f, 'read_file', { project: seed.indexDocId, path: 'a.qmd' }),
      );
      expect(reread.content).toBe('v2\n');
      expect(reread.hash).toBe(written.hash);
    } finally {
      await f.close();
    }
  });

  it('write_file with a stale expected_hash is refused with the current content + hash and changes nothing', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const staleRead = parseJson(
        await callTool(f, 'read_file', { project: seed.indexDocId, path: 'a.qmd' }),
      );
      // A collaborator (or an earlier agent step) edits the file.
      const collab = await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        content: 'collaborator edit\n',
      });
      expect(collab.isError).not.toBe(true);

      const refused = await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        content: 'my stale overwrite\n',
        expected_hash: staleRead.hash,
      });
      expect(refused.isError).toBe(true);
      const payload = parseJson(refused);
      // The refusal carries the CURRENT content + hash so the agent can
      // merge without an extra read.
      expect(payload.hash).toMatch(HASH_RE);
      expect(payload.hash).not.toBe(staleRead.hash);
      expect(payload.content).toBe('collaborator edit\n');

      // Nothing changed on the hub.
      const after = parseJson(
        await callTool(f, 'read_file', { project: seed.indexDocId, path: 'a.qmd' }),
      );
      expect(after.content).toBe('collaborator edit\n');
    } finally {
      await f.close();
    }
  });

  it('patch_file with a matching expected_hash succeeds and returns the new hash', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'the qick brown fox\n' }]);
      const read = parseJson(
        await callTool(f, 'read_file', { project: seed.indexDocId, path: 'a.qmd' }),
      );

      const patched = await callTool(f, 'patch_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        old_string: 'qick',
        new_string: 'quick',
        expected_hash: read.hash,
      });
      expect(patched.isError).not.toBe(true);
      const payload = parseJson(patched);
      expect(payload.hash).toMatch(HASH_RE);
      expect(payload.hash).not.toBe(read.hash);

      const reread = parseJson(
        await callTool(f, 'read_file', { project: seed.indexDocId, path: 'a.qmd' }),
      );
      expect(reread.content).toBe('the quick brown fox\n');
      expect(reread.hash).toBe(payload.hash);
    } finally {
      await f.close();
    }
  });

  it('patch_file with a stale expected_hash is refused and splices nothing', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const staleRead = parseJson(
        await callTool(f, 'read_file', { project: seed.indexDocId, path: 'a.qmd' }),
      );
      const collab = await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        content: 'v2 other\n',
      });
      expect(collab.isError).not.toBe(true);

      const refused = await callTool(f, 'patch_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        old_string: 'v1',
        new_string: 'v2 mine',
        expected_hash: staleRead.hash,
      });
      expect(refused.isError).toBe(true);
      const payload = parseJson(refused);
      expect(payload.content).toBe('v2 other\n');
      expect(payload.hash).not.toBe(staleRead.hash);

      const after = parseJson(
        await callTool(f, 'read_file', { project: seed.indexDocId, path: 'a.qmd' }),
      );
      expect(after.content).toBe('v2 other\n');
    } finally {
      await f.close();
    }
  });
});
