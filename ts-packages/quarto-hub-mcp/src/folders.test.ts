/**
 * Folder lifecycle (CAP-6, resolves HY-4's undocumented asymmetry):
 * `create_folder` / `delete_folder` manage explicit folder markers;
 * `list_files` includes them as `{ path, type: 'folder' }` entries.
 *
 * Folders in Quarto Hub are markers, not containers: a file at
 * `a/b/c.qmd` needs no folder to exist, and deleting a folder marker
 * cannot orphan files. The MCP tool adds the guard a human expects:
 * delete refuses while files remain under the path unless
 * `recursive: true`, which deletes the contained files first.
 */

import { describe, it, expect } from 'vitest';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';

interface ListedEntry {
  path: string;
  type?: string;
  status?: string;
}

function structuredOf(result: { structuredContent?: unknown }): Record<string, unknown> {
  const sc = result.structuredContent;
  if (sc === undefined || sc === null || typeof sc !== 'object') {
    throw new Error(`expected structuredContent, got: ${JSON.stringify(result)}`);
  }
  return sc as Record<string, unknown>;
}

async function listPaths(
  f: InMemoryMcpFixture,
  project: string,
): Promise<ListedEntry[]> {
  const listed = (await callTool(f, 'list_files', { project })).structuredContent as {
    files: ListedEntry[];
  };
  return listed.files;
}

describe('create_folder (CAP-6)', () => {
  it('creates a folder that list_files shows as a folder entry', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'create_folder', {
        project: seed.indexDocId,
        path: 'assets/images',
      });
      expect(result.isError).not.toBe(true);
      expect(structuredOf(result)).toMatchObject({
        path: 'assets/images',
        created: true,
        synced: true,
      });

      const entries = await listPaths(f, seed.indexDocId);
      const folder = entries.find((e) => e.path === 'assets/images');
      expect(folder?.type).toBe('folder');
      // Files still list normally.
      expect(entries.find((e) => e.path === 'index.qmd')?.type).toBe('text');
    } finally {
      await f.close();
    }
  });

  it('is idempotent: re-creating reports created: false', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, []);
      await callTool(f, 'create_folder', { project: seed.indexDocId, path: 'docs' });
      const again = await callTool(f, 'create_folder', { project: seed.indexDocId, path: 'docs' });
      expect(again.isError).not.toBe(true);
      expect(structuredOf(again)).toMatchObject({ path: 'docs', created: false });
    } finally {
      await f.close();
    }
  });

  it('refuses a path already taken by a file', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'create_folder', {
        project: seed.indexDocId,
        path: 'index.qmd',
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type === 'text') {
        expect(block.text).toMatch(/file/i);
      }
    } finally {
      await f.close();
    }
  });

  it('folders implied by file paths are NOT listed — only explicit markers', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a/b/c.qmd', content: 'x\n' }]);
      const entries = await listPaths(f, seed.indexDocId);
      expect(entries.filter((e) => e.type === 'folder')).toEqual([]);
    } finally {
      await f.close();
    }
  });
});

describe('delete_folder (CAP-6)', () => {
  it('deletes an empty folder', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, []);
      await callTool(f, 'create_folder', { project: seed.indexDocId, path: 'scratch' });
      const result = await callTool(f, 'delete_folder', {
        project: seed.indexDocId,
        path: 'scratch',
      });
      expect(result.isError).not.toBe(true);
      expect(structuredOf(result)).toMatchObject({
        path: 'scratch',
        deleted: true,
        synced: true,
      });
      expect(
        (await listPaths(f, seed.indexDocId)).find((e) => e.path === 'scratch'),
      ).toBeUndefined();
    } finally {
      await f.close();
    }
  });

  it('refuses a non-empty folder without recursive, naming contained files', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [
        { path: 'docs/a.qmd', content: 'a\n' },
        { path: 'docs/b.qmd', content: 'b\n' },
      ]);
      await callTool(f, 'create_folder', { project: seed.indexDocId, path: 'docs' });
      const result = await callTool(f, 'delete_folder', {
        project: seed.indexDocId,
        path: 'docs',
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type === 'text') {
        expect(block.text).toMatch(/recursive/);
        expect(block.text).toMatch(/docs\/a\.qmd/);
      }
      // Nothing deleted.
      expect((await listPaths(f, seed.indexDocId)).length).toBe(3);
    } finally {
      await f.close();
    }
  });

  it('recursive deletes contained files, then the marker', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [
        { path: 'docs/a.qmd', content: 'a\n' },
        { path: 'docs/b.qmd', content: 'b\n' },
        { path: 'keep.qmd', content: 'k\n' },
      ]);
      await callTool(f, 'create_folder', { project: seed.indexDocId, path: 'docs' });
      const result = await callTool(f, 'delete_folder', {
        project: seed.indexDocId,
        path: 'docs',
        recursive: true,
      });
      expect(result.isError).not.toBe(true);
      expect(structuredOf(result)).toMatchObject({
        path: 'docs',
        deleted: true,
        files_deleted: 2,
        synced: true,
      });
      const remaining = (await listPaths(f, seed.indexDocId)).map((e) => e.path);
      expect(remaining).toEqual(['keep.qmd']);
    } finally {
      await f.close();
    }
  });

  it('errors on a folder that does not exist', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, []);
      const result = await callTool(f, 'delete_folder', {
        project: seed.indexDocId,
        path: 'nope',
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type === 'text') {
        expect(block.text).toMatch(/nope/);
        expect(block.text).toMatch(/list_files/);
      }
    } finally {
      await f.close();
    }
  });
});
