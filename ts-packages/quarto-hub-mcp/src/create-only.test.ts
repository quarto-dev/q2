/**
 * `write_file` with `create_only: true` — the create-with-refusal arm
 * (Phase 7, bd-1ilystml). Phase 7 folded `create_file` into `write_file`
 * (ERG-5 rule (a): same verb, varied by parameter — the same rule that
 * put binary I/O on `write_file` in Phase 2). These tests pin what the
 * folded tool must keep doing:
 *
 * - absent path → creates, result carries `created: true` and the hash;
 * - existing path → refused, naming the file (never an overwrite);
 * - the base64 arm honors `create_only` too;
 * - `create_only` + `expected_hash` is a usage error (contradictory:
 *   `expected_hash` presumes the file exists).
 */

import { describe, it, expect } from 'vitest';
import { createHash } from 'node:crypto';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
} from './in-memory-fixture.js';

const PDF = new Uint8Array([0x25, 0x50, 0x44, 0x46, 0x2d, 0x31, 0x2e, 0x37, 0x0a, 0x25, 0xe2]);
const b64 = (bytes: Uint8Array) => Buffer.from(bytes).toString('base64');
const sha256Hex = (s: string) => `sha256:${createHash('sha256').update(s, 'utf8').digest('hex')}`;

function structuredOf(result: { structuredContent?: unknown }): Record<string, unknown> {
  const sc = result.structuredContent;
  if (sc === undefined || typeof sc !== 'object') {
    throw new Error('expected structuredContent on the result');
  }
  return sc as Record<string, unknown>;
}

describe('edit/write create_only (Phase 7)', () => {
  it('creates a text file when the path is absent', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, []);
      const result = await callTool(f, 'edit', { action: 'write',
        project: seed.indexDocId,
        path: 'new-file.qmd',
        content: 'Brand new file',
        create_only: true,
      });
      expect(result.isError).not.toBe(true);
      const created = structuredOf(result);
      expect(created.created).toBe(true);
      expect(created.hash).toBe(sha256Hex('Brand new file'));

      const read = await callTool(f, 'read', { action: 'read',
        project: seed.indexDocId,
        path: 'new-file.qmd',
      });
      expect(structuredOf(read).content).toBe('Brand new file');
    } finally {
      await f.close();
    }
  });

  it('refuses when the file already exists', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'hello.qmd', content: 'original\n' }]);
      const result = await callTool(f, 'edit', { action: 'write',
        project: seed.indexDocId,
        path: 'hello.qmd',
        content: 'duplicate',
        create_only: true,
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type !== 'text') throw new Error('unreachable');
      expect(block.text).toContain('already exists');
      expect(block.text).toContain('hello.qmd');

      // The refusal never touched the content.
      const read = await callTool(f, 'read', { action: 'read',
        project: seed.indexDocId,
        path: 'hello.qmd',
      });
      expect(structuredOf(read).content).toBe('original\n');
    } finally {
      await f.close();
    }
  });

  it('honors create_only on the base64 arm', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, []);
      const created = await callTool(f, 'edit', { action: 'write',
        project: seed.indexDocId,
        path: 'paper.pdf',
        content: b64(PDF),
        encoding: 'base64',
        create_only: true,
      });
      expect(created.isError).not.toBe(true);
      const meta = structuredOf(created);
      expect(meta.created).toBe(true);
      expect(meta.mimeType).toBe('application/pdf');

      const refused = await callTool(f, 'edit', { action: 'write',
        project: seed.indexDocId,
        path: 'paper.pdf',
        content: b64(PDF),
        encoding: 'base64',
        create_only: true,
      });
      expect(refused.isError).toBe(true);
      const block = refused.content[0];
      if (block?.type !== 'text') throw new Error('unreachable');
      expect(block.text).toContain('already exists');
    } finally {
      await f.close();
    }
  });

  it('rejects create_only combined with expected_hash', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, []);
      const result = await callTool(f, 'edit', { action: 'write',
        project: seed.indexDocId,
        path: 'new.qmd',
        content: 'x',
        create_only: true,
        expected_hash: sha256Hex('x'),
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type !== 'text') throw new Error('unreachable');
      expect(block.text).toMatch(/create_only.*expected_hash|expected_hash.*create_only/);
    } finally {
      await f.close();
    }
  });

  it('create_file is gone from the surface', async () => {
    const f = await startInMemoryMcp();
    try {
      const { tools } = await f.client.listTools();
      expect(tools.map((t) => t.name)).not.toContain('create_file');
      expect(tools.map((t) => t.name)).not.toContain('list_presence');
    } finally {
      await f.close();
    }
  });
});
