/**
 * `list_projects` (CAP-3 MVP): enumerate a project-set document passed
 * by doc id or share URL. A project-set is how the web client groups a
 * user's projects (collections); full "list my projects" without a set
 * id is the Phase 6 hub-side registry, out of scope here.
 *
 * bd-3h5hbfdv: `project_set` is optional — the first explicit call that
 * reads successfully is remembered in the OS keyring as the default, and
 * `set_default_project_set` overwrites it. The tests below inject an
 * in-memory keyring backend.
 */

import { describe, it, expect } from 'vitest';

import type { ProjectSetDocument, ProjectSetEntry } from '@quarto/quarto-sync-client';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
  MemoryBackend,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';
import { parseProjectRef } from './share-url.js';

function makeEntry(overrides: Partial<ProjectSetEntry>): ProjectSetEntry {
  return {
    indexDocId: 'automerge:placeholder',
    syncServer: 'wss://quarto-hub.com/ws',
    description: 'A project',
    addedAt: '2026-10-01T00:00:00.000Z',
    lastAccessed: '2026-10-02T00:00:00.000Z',
    ...overrides,
  };
}

function seedSet(f: InMemoryMcpFixture, doc: ProjectSetDocument): string {
  const handle = f.hub.repo.create<ProjectSetDocument>();
  handle.change((d) => {
    d.version = doc.version;
    d.projects = doc.projects;
    if (doc.name !== undefined) d.name = doc.name;
  });
  return handle.documentId;
}

const SET_DOC: ProjectSetDocument = {
  version: 1,
  name: 'Charlie’s collection',
  projects: {
    projOlder: makeEntry({
      indexDocId: 'automerge:projOlder',
      description: 'Older book',
      lastAccessed: '2026-09-30T00:00:00.000Z',
    }),
    projNewer: makeEntry({
      indexDocId: 'automerge:projNewer',
      description: 'Newer paper',
      lastAccessed: '2026-10-05T00:00:00.000Z',
    }),
  },
};

interface ListedProject {
  indexDocId: string;
  syncServer: string;
  description: string;
  lastAccessed: string;
  shareUrl: string;
}

describe('project/list_sets (CAP-3)', () => {
  it('enumerates a project-set by doc id, most-recently-accessed first', async () => {
    const f = await startInMemoryMcp();
    try {
      const setId = seedSet(f, SET_DOC);
      const result = await callTool(f, 'project', { action: 'list_sets', project_set: setId });
      expect(result.isError).not.toBe(true);
      const sc = result.structuredContent as { name?: string; projects: ListedProject[] };
      expect(sc.name).toBe('Charlie’s collection');
      expect(sc.projects.map((p) => p.indexDocId)).toEqual(['projNewer', 'projOlder']);
      expect(sc.projects[0]).toMatchObject({
        syncServer: 'wss://quarto-hub.com/ws',
        description: 'Newer paper',
      });
      // Every entry carries a complete share URL — hub-client requires
      // server/file/name (bd-jtl4o0pt) — that round-trips to the project.
      // Entries carry no summary cache, so file= is the index.qmd fallback.
      for (const p of sc.projects) {
        const ref = parseProjectRef(p.shareUrl);
        expect(ref.project).toBe(p.indexDocId);
        expect(ref.server).toBe(p.syncServer);
        expect(ref.name).toBe(p.description);
        expect(ref.file).toBe('index.qmd');
      }
    } finally {
      await f.close();
    }
  });

  it('accepts a share URL (server= routes the read)', async () => {
    const f = await startInMemoryMcp();
    try {
      const setId = seedSet(f, SET_DOC);
      const shareUrl =
        `https://quarto-hub.com/#/share/${setId}?server=${encodeURIComponent(f.hub.url)}`;
      const result = await callTool(f, 'project', { action: 'list_sets', project_set: shareUrl });
      expect(result.isError).not.toBe(true);
      const sc = result.structuredContent as { projects: ListedProject[] };
      expect(sc.projects.length).toBe(2);
    } finally {
      await f.close();
    }
  });

  it('accepts a link-project-set URL (the web client\'s copy-link form)', async () => {
    const f = await startInMemoryMcp();
    try {
      const setId = seedSet(f, SET_DOC);
      const linkUrl =
        `https://quarto-hub.com/#/link-project-set/${setId}` +
        `?server=${encodeURIComponent(f.hub.url)}`;
      const result = await callTool(f, 'project', { action: 'list_sets', project_set: linkUrl });
      expect(result.isError).not.toBe(true);
      const sc = result.structuredContent as { projects: ListedProject[] };
      expect(sc.projects.length).toBe(2);
    } finally {
      await f.close();
    }
  });

  it('a project index id is not a project set — actionable error', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'project', { action: 'list_sets', project_set: seed.indexDocId });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type === 'text') {
        expect(block.text).toMatch(/not a project-set/i);
        expect(block.text).toMatch(/action: "connect"/);
      }
    } finally {
      await f.close();
    }
  });
});

// ---------------------------------------------------------------------------
// bd-3h5hbfdv: the remembered default project set
// ---------------------------------------------------------------------------

const SET_B_DOC: ProjectSetDocument = {
  version: 1,
  name: 'Second set',
  projects: {
    projOnly: makeEntry({ indexDocId: 'automerge:projOnly', description: 'Only project' }),
  },
};

describe('default project set (bd-3h5hbfdv)', () => {
  it('a bare call with no stored default instructs the caller to provide one', async () => {
    const f = await startInMemoryMcp({ projectSetBackend: new MemoryBackend() });
    try {
      const result = await callTool(f, 'project', { action: 'list_sets', });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type === 'text') {
        // An instruction to the agent: supply a set now (asking the user
        // if it isn't known)…
        expect(block.text).toMatch(/provide/i);
        expect(block.text).toMatch(/ask the user/i);
        // …and the promise that doing so once is enough.
        expect(block.text).toMatch(/stores? it .*default/i);
        expect(block.text).toMatch(/subsequent/i);
        expect(block.text).toMatch(/link-project-set/);
        expect(block.text).toMatch(/set_default_set/);
      }
    } finally {
      await f.close();
    }
  });

  it('the first explicit call persists its argument; a bare call then uses it', async () => {
    const backend = new MemoryBackend();
    const f = await startInMemoryMcp({ projectSetBackend: backend });
    try {
      const setId = seedSet(f, SET_DOC);
      const link =
        `https://quarto-hub.com/#/link-project-set/${setId}` +
        `?server=${encodeURIComponent(f.hub.url)}`;
      const explicit = await callTool(f, 'project', { action: 'list_sets', project_set: link });
      expect(explicit.isError).not.toBe(true);
      // Persisted verbatim — server= and all — so a bare call routes the same way.
      expect(backend.value).toBe(link);

      const bare = await callTool(f, 'project', { action: 'list_sets', });
      expect(bare.isError).not.toBe(true);
      const sc = bare.structuredContent as { name?: string; projects: ListedProject[] };
      expect(sc.name).toBe('Charlie’s collection');
      expect(sc.projects.length).toBe(2);
    } finally {
      await f.close();
    }
  });

  it('an explicit different set does not overwrite the stored default', async () => {
    const backend = new MemoryBackend();
    const f = await startInMemoryMcp({ projectSetBackend: backend });
    try {
      const setA = seedSet(f, SET_DOC);
      const setB = seedSet(f, SET_B_DOC);
      await callTool(f, 'project', { action: 'list_sets', project_set: setA });
      expect(backend.value).toBe(setA);

      const other = await callTool(f, 'project', { action: 'list_sets', project_set: setB });
      expect(other.isError).not.toBe(true);
      expect((other.structuredContent as { name?: string }).name).toBe('Second set');
      // Exploring another set must not switch the user's default.
      expect(backend.value).toBe(setA);
    } finally {
      await f.close();
    }
  });

  it('a failed read does not persist anything', async () => {
    const backend = new MemoryBackend();
    const f = await startInMemoryMcp({ projectSetBackend: backend });
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'project', { action: 'list_sets', project_set: seed.indexDocId });
      expect(result.isError).toBe(true);
      expect(backend.value).toBeNull();
    } finally {
      await f.close();
    }
  });

  it('a keyring write failure degrades to a warning, not a failed listing', async () => {
    const backend = new MemoryBackend();
    backend.failWrites = true;
    const f = await startInMemoryMcp({ projectSetBackend: backend });
    try {
      const setId = seedSet(f, SET_DOC);
      const result = await callTool(f, 'project', { action: 'list_sets', project_set: setId });
      expect(result.isError).not.toBe(true);
      const warning = result.content.find(
        (b) => b.type === 'text' && /could not remember/i.test(b.text),
      );
      expect(warning).toBeDefined();
    } finally {
      await f.close();
    }
  });

  it('project/set_default_set validates before storing, then overwrites', async () => {
    const backend = new MemoryBackend();
    const f = await startInMemoryMcp({ projectSetBackend: backend });
    try {
      const setA = seedSet(f, SET_DOC);
      const setB = seedSet(f, SET_B_DOC);
      await callTool(f, 'project', { action: 'list_sets', project_set: setA });
      expect(backend.value).toBe(setA);

      // A project index id is rejected and the default is left alone.
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const bad = await callTool(f, 'project', { action: 'set_default_set',
        project_set: seed.indexDocId,
      });
      expect(bad.isError).toBe(true);
      expect(backend.value).toBe(setA);

      // The manual update.
      const ok = await callTool(f, 'project', { action: 'set_default_set', project_set: setB });
      expect(ok.isError).not.toBe(true);
      expect(backend.value).toBe(setB);
      const sc = ok.structuredContent as {
        default_project_set: string;
        name?: string;
        project_count: number;
      };
      expect(sc.default_project_set).toBe(setB);
      expect(sc.name).toBe('Second set');
      expect(sc.project_count).toBe(1);

      const bare = await callTool(f, 'project', { action: 'list_sets', });
      expect((bare.structuredContent as { name?: string }).name).toBe('Second set');
    } finally {
      await f.close();
    }
  });

  it('a stale stored default errors with a pointer to project/set_default_set', async () => {
    const backend = new MemoryBackend();
    const f = await startInMemoryMcp({ projectSetBackend: backend });
    try {
      // The stored default names something that is not a project set (a
      // deleted set reads the same way from the caller's side).
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      backend.value = seed.indexDocId;
      const result = await callTool(f, 'project', { action: 'list_sets', });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type === 'text') {
        expect(block.text).toMatch(/remembered default/);
        expect(block.text).toMatch(/set_default_set/);
      }
    } finally {
      await f.close();
    }
  });

  it('project/set_default_set reports a keyring write failure as an error', async () => {
    const backend = new MemoryBackend();
    backend.failWrites = true;
    const f = await startInMemoryMcp({ projectSetBackend: backend });
    try {
      const setId = seedSet(f, SET_DOC);
      const result = await callTool(f, 'project', { action: 'set_default_set', project_set: setId });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type === 'text') {
        expect(block.text).toMatch(/could not store/i);
      }
    } finally {
      await f.close();
    }
  });
});
