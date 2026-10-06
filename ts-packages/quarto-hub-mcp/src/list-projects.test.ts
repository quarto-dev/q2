/**
 * `list_projects` (CAP-3 MVP): enumerate a project-set document passed
 * by doc id or share URL. A project-set is how the web client groups a
 * user's projects (collections); full "list my projects" without a set
 * id is the Phase 6 hub-side registry, out of scope here.
 */

import { describe, it, expect } from 'vitest';

import type { ProjectSetDocument, ProjectSetEntry } from '@quarto/quarto-sync-client';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
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

describe('list_projects (CAP-3)', () => {
  it('enumerates a project-set by doc id, most-recently-accessed first', async () => {
    const f = await startInMemoryMcp();
    try {
      const setId = seedSet(f, SET_DOC);
      const result = await callTool(f, 'list_projects', { project_set: setId });
      expect(result.isError).not.toBe(true);
      const sc = result.structuredContent as { name?: string; projects: ListedProject[] };
      expect(sc.name).toBe('Charlie’s collection');
      expect(sc.projects.map((p) => p.indexDocId)).toEqual(['projNewer', 'projOlder']);
      expect(sc.projects[0]).toMatchObject({
        syncServer: 'wss://quarto-hub.com/ws',
        description: 'Newer paper',
      });
      // Every entry carries a share URL that round-trips to the project.
      for (const p of sc.projects) {
        expect(parseProjectRef(p.shareUrl).project).toBe(p.indexDocId);
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
      const result = await callTool(f, 'list_projects', { project_set: shareUrl });
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
      const result = await callTool(f, 'list_projects', { project_set: seed.indexDocId });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type === 'text') {
        expect(block.text).toMatch(/not a project-set/i);
        expect(block.text).toMatch(/connect_project/);
      }
    } finally {
      await f.close();
    }
  });
});
