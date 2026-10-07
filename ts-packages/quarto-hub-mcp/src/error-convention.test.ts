/**
 * Actionable error convention (ERG-4) and the phantom-tool fix (HY-1),
 * bd-zv8u2sxi; claude-notes/plans/2026-10-05-elevate-quarto-hub-mcp.md.
 *
 * `isError` results exist "to enable model self-correction" (MCP tools
 * spec) — so every data-tool error names the failing parameter, the
 * current state, and the next tool to call, and path errors list up to
 * three closest existing paths. HY-1: binary reads ride read_file
 * itself (CAP-4) — nothing may send the agent to
 * `read_binary_file_metadata`, a tool that does not exist.
 */

import { describe, it, expect } from 'vitest';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
} from './in-memory-fixture.js';

function textOf(result: { content: Array<{ type: string; text?: string }> }): string {
  const block = result.content[0];
  if (block?.type !== 'text' || typeof block.text !== 'string') {
    throw new Error(`expected a text content block, got ${JSON.stringify(block)}`);
  }
  return block.text;
}

describe('actionable errors (ERG-4, HY-1)', () => {
  it('read_file on a missing path names list_files and the closest existing paths', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [
        { path: 'docs/index.qmd', content: 'a\n' },
        { path: 'docs/guide.qmd', content: 'b\n' },
        { path: 'index.qmd', content: 'c\n' },
      ]);
      const result = await callTool(f, 'read_file', {
        project: seed.indexDocId,
        path: 'docs/indx.qmd',
      });
      expect(result.isError).toBe(true);
      const text = textOf(result);
      // The failing parameter and the state.
      expect(text).toContain('docs/indx.qmd');
      // The next tool to call.
      expect(text).toContain('list_files');
      // Up to three closest existing paths, best first.
      expect(text).toContain('docs/index.qmd');
    } finally {
      await f.close();
    }
  });

  it('read_file reads binary files in place — the phantom read_binary_file_metadata era is over (HY-1, CAP-4)', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'x\n' }]);
      // Seed a binary doc directly through the manager's sync client.
      const state = await f.manager.connect(seed.indexDocId);
      await state.client.createBinaryFile(
        'img.png',
        new Uint8Array([137, 80, 78, 71]),
        'image/png',
      );
      const result = await callTool(f, 'read_file', {
        project: seed.indexDocId,
        path: 'img.png',
      });
      // CAP-4: binary reads succeed through read_file itself; no error,
      // and certainly no pointer to a tool that does not exist.
      expect(result.isError).not.toBe(true);
      expect(JSON.stringify(result.content)).not.toContain('read_binary_file_metadata');
      expect(result.structuredContent).toMatchObject({ path: 'img.png', type: 'binary' });
    } finally {
      await f.close();
    }
  });

  it('patch_file with an unmatched old_string names the parameter and points at read_file', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'current text\n' }]);
      const result = await callTool(f, 'patch_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        old_string: 'stale text',
        new_string: 'replacement',
      });
      expect(result.isError).toBe(true);
      const text = textOf(result);
      expect(text).toContain('old_string');
      expect(text).toContain('read_file');
    } finally {
      await f.close();
    }
  });

  it('rename_file onto an existing destination names the state and the way forward', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [
        { path: 'a.qmd', content: 'a\n' },
        { path: 'b.qmd', content: 'b\n' },
      ]);
      const result = await callTool(f, 'rename_file', {
        project: seed.indexDocId,
        old_path: 'a.qmd',
        new_path: 'b.qmd',
      });
      expect(result.isError).toBe(true);
      const text = textOf(result);
      expect(text).toContain('b.qmd');
      // The way forward: a different name, or remove the destination.
      expect(text).toMatch(/delete_file|different/);
    } finally {
      await f.close();
    }
  });
});
