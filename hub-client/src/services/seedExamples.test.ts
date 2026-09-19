/**
 * seedExampleProjects (bd-3fwtdhil): on a brand-new browser, create the
 * "Examples / Templates" collection and fill it with every project choice
 * the registry flags as `seed`, in registry order. Pure orchestration over
 * injected dependencies, so no WASM, IndexedDB, or sync server is needed.
 *
 * Plan: claude-notes/plans/2026-09-17-get-started-collection.md
 */
import { describe, it, expect, vi } from 'vitest';
import {
  seedExampleProjects,
  SEED_COLLECTION_NAME,
  type SeedDeps,
  type SeedChoice,
} from './seedExamples';

const CHOICES: SeedChoice[] = [
  { id: 'default', name: 'Default', description: 'A minimal Quarto project' },
  { id: 'example-meeting-notes', name: 'Meeting Notes', description: 'd', seed: true },
  { id: 'example-website', name: 'Website', description: 'd', seed: true },
  { id: 'hub-placeholder', name: 'Welcome', description: 'd', seed: false },
  { id: 'example-article', name: 'Article', description: 'd', seed: true },
];

function makeDeps(overrides: Partial<SeedDeps> = {}): SeedDeps {
  let n = 0;
  return {
    syncServer: 'wss://hub.example/ws',
    resolveSyncServerUrl: (s) => `${s}#resolved`,
    getProjectChoices: vi.fn(async () => CHOICES),
    createProject: vi.fn(async (id: string) => ({
      success: true,
      files: [
        { path: 'index.qmd', content_type: 'text' as const, content: `# ${id}` },
        { path: 'logo.svg', content_type: 'text' as const, content: '<svg/>', mime_type: 'image/svg+xml' },
      ],
    })),
    createNewProject: vi.fn(async () => ({ indexDocId: `automerge:doc${++n}` })),
    addLocalProject: vi.fn(async () => {}),
    createCollection: vi.fn(async () => 'automerge:collection'),
    addProjectToCollection: vi.fn(),
    log: vi.fn(),
    ...overrides,
  };
}

describe('seedExampleProjects', () => {
  it('creates the Examples / Templates collection and seeds the flagged choices in order', async () => {
    const deps = makeDeps();
    const result = await seedExampleProjects(deps);

    expect(SEED_COLLECTION_NAME).toBe('Examples / Templates');
    expect(deps.createCollection).toHaveBeenCalledTimes(1);
    expect(deps.createCollection).toHaveBeenCalledWith(SEED_COLLECTION_NAME);

    expect(vi.mocked(deps.createProject).mock.calls).toEqual([
      ['example-meeting-notes', 'Meeting Notes'],
      ['example-website', 'Website'],
      ['example-article', 'Article'],
    ]);

    expect(result).toEqual({
      collectionDocId: 'automerge:collection',
      seeded: ['example-meeting-notes', 'example-website', 'example-article'],
      failed: [],
    });
  });

  it('writes each project to the resolved server, stores the portable server, and files it in the collection', async () => {
    const deps = makeDeps();
    await seedExampleProjects(deps);

    const created = vi.mocked(deps.createNewProject).mock.calls[0][0];
    expect(created.syncServer).toBe('wss://hub.example/ws#resolved');
    expect(created.files).toEqual([
      { path: 'index.qmd', content: '# example-meeting-notes', contentType: 'text', mimeType: undefined },
      { path: 'logo.svg', content: '<svg/>', contentType: 'text', mimeType: 'image/svg+xml' },
    ]);

    expect(deps.addLocalProject).toHaveBeenCalledWith('automerge:doc1', 'wss://hub.example/ws', 'Meeting Notes');
    expect(vi.mocked(deps.addProjectToCollection).mock.calls).toEqual([
      ['automerge:collection', { indexDocId: 'automerge:doc1', syncServer: 'wss://hub.example/ws', description: 'Meeting Notes' }],
      ['automerge:collection', { indexDocId: 'automerge:doc2', syncServer: 'wss://hub.example/ws', description: 'Website' }],
      ['automerge:collection', { indexDocId: 'automerge:doc3', syncServer: 'wss://hub.example/ws', description: 'Article' }],
    ]);
  });

  it('skips a project whose scaffold fails and keeps going', async () => {
    const deps = makeDeps({
      createProject: vi.fn(async (id: string) =>
        id === 'example-website'
          ? { success: false, error: 'no scaffold' }
          : { success: true, files: [{ path: 'index.qmd', content_type: 'text' as const, content: id }] },
      ),
    });
    const result = await seedExampleProjects(deps);
    expect(result?.seeded).toEqual(['example-meeting-notes', 'example-article']);
    expect(result?.failed).toEqual(['example-website']);
    expect(deps.addProjectToCollection).toHaveBeenCalledTimes(2);
    expect(deps.log).toHaveBeenCalledWith(expect.stringContaining('example-website'), expect.anything());
  });

  it('skips a project whose document creation throws and keeps going', async () => {
    let n = 0;
    const deps = makeDeps({
      createNewProject: vi.fn(async () => {
        n += 1;
        if (n === 1) throw new Error('sync server refused');
        return { indexDocId: `automerge:doc${n}` };
      }),
    });
    const result = await seedExampleProjects(deps);
    expect(result?.seeded).toEqual(['example-website', 'example-article']);
    expect(result?.failed).toEqual(['example-meeting-notes']);
  });

  it('does nothing when no choice is flagged as seed', async () => {
    const deps = makeDeps({
      getProjectChoices: vi.fn(async () => [{ id: 'default', name: 'Default', description: 'd' }]),
    });
    const result = await seedExampleProjects(deps);
    expect(result).toBeNull();
    expect(deps.createCollection).not.toHaveBeenCalled();
    expect(deps.createProject).not.toHaveBeenCalled();
  });

  it('gives up cleanly when the collection cannot be created', async () => {
    const deps = makeDeps({
      createCollection: vi.fn(async () => {
        throw new Error('Not connected');
      }),
    });
    const result = await seedExampleProjects(deps);
    expect(result).toBeNull();
    expect(deps.createProject).not.toHaveBeenCalled();
    expect(deps.log).toHaveBeenCalledWith(expect.stringContaining('collection'), expect.anything());
  });
});
