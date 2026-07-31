import { expect, it, vi } from 'vitest';
const mocks = vi.hoisted(() => {
  const handle = {
    documentId: 'offline-root-doc',
    doc: vi.fn(() => ({ projects: {}, version: 1 })),
    on: vi.fn(),
    off: vi.fn(),
  };
  const repo = {
    import: vi.fn(() => handle),
    flush: vi.fn(async () => {}),
    networkSubsystem: { on: vi.fn() },
  };

  return {
    repo,
    from: vi.fn((document: Record<string, unknown>) => document),
    save: vi.fn(() => new Uint8Array([1, 2, 3])),
  };
});
vi.mock('@automerge/automerge-repo', () => ({
  Repo: class {
    import = mocks.repo.import;
    flush = mocks.repo.flush;
    networkSubsystem = mocks.repo.networkSubsystem;
  },
}));
vi.mock('@automerge/automerge-repo-network-websocket', () => ({
  BrowserWebSocketClientAdapter: class { disconnect = vi.fn(); },
}));
vi.mock('@automerge/automerge-repo-storage-indexeddb', () => ({
  IndexedDBStorageAdapter: class {},
}));
vi.mock('@automerge/automerge', () => ({
  from: mocks.from,
  save: mocks.save,
}));
import { createProjectSet } from './projectSetService';
it('imports an empty personal root locally while sync is offline', async () => {
  await expect(
    createProjectSet('wss://offline.example/ws', 'My projects'),
  ).resolves.toBe('offline-root-doc');
  expect(mocks.from).toHaveBeenCalledWith({
    projects: {},
    version: 1,
    name: 'My projects',
  });
  expect(mocks.repo.import).toHaveBeenCalledWith(new Uint8Array([1, 2, 3]));
  expect(mocks.repo.flush).toHaveBeenCalledWith(['offline-root-doc']);
});
